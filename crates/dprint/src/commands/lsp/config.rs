use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Context;
use anyhow::Result;

use crate::cache::RemoteCacheMode;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::get_config_file_base_dir;
use crate::configuration::get_default_config_file_in_ancestor_directories;
use crate::configuration::resolve_config_from_path_with_ancestors;
use crate::configuration::resolve_global_config_path_and_text_at_path_root;
use crate::environment::Environment;
use crate::plugins;
use crate::resolution::PluginsScope;
use crate::resolution::resolve_plugins_scope;
use crate::utils::AsyncMutex;
use crate::utils::PathSource;

type ScopeCell<TEnvironment> = AsyncMutex<Option<CachedScope<TEnvironment>>>;

struct CachedScope<TEnvironment: Environment> {
  scope: Rc<PluginsScope<TEnvironment>>,
  /// The generation of the plugins the scope was resolved with.
  plugins_generation: usize,
}

pub struct LspPluginsScopeContainer<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
  plugins_scope_by_config: RefCell<HashMap<String, Rc<ScopeCell<TEnvironment>>>>,
  config_override: Option<PathBuf>,
  remote_cache_mode: RemoteCacheMode,
  /// Incremented each time the plugins are shut down, which is when the
  /// scopes that were resolved before then can't be used anymore.
  plugins_generation: Cell<usize>,
}

impl<TEnvironment: Environment> LspPluginsScopeContainer<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
    config_override: Option<PathBuf>,
    remote_cache_mode: RemoteCacheMode,
  ) -> Self {
    Self {
      environment,
      plugin_resolver,
      plugins_scope_by_config: Default::default(),
      config_override,
      remote_cache_mode,
      plugins_generation: Default::default(),
    }
  }

  pub async fn shutdown(&self) {
    self.plugins_scope_by_config.borrow_mut().clear();
    self.plugin_resolver.clear_and_shutdown_initialized().await;
  }

  /// Resolves the plugins to format the files in the provided directory with.
  /// `use_global_config` is for using the global config file when there's no
  /// config file in an ancestor directory.
  pub async fn resolve_by_path(&self, dir_path: &Path, use_global_config: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let config_file_bytes = if let Some(path) = &self.config_override {
      let path = self.environment.canonicalize(path).context("failed resolving --config path")?;
      let content = self.environment.read_file(&path).context("failed resolving --config path")?;
      Some(ResolvedConfigPathWithText {
        base_path: get_config_file_base_dir(&self.environment, &path).unwrap_or_else(|| path.clone()),
        source: PathSource::new_local(path),
        is_first_download: false,
        content,
        is_global_config: false,
      })
    } else {
      match get_default_config_file_in_ancestor_directories(&self.environment, dir_path)? {
        Some(config) => Some(config),
        None if use_global_config => resolve_global_config_path_and_text_at_path_root(&self.environment, dir_path)?,
        None => None,
      }
    };
    let Some(config_file_bytes) = config_file_bytes else {
      return Ok(None);
    };
    let cell = {
      // the base path is part of the key because the global config
      // file has a different one for each root directory (ex. drive)
      let key = format!("{}\n{}", config_file_bytes.source.display(), config_file_bytes.base_path.display());
      let mut plugins_scope_by_config = self.plugins_scope_by_config.borrow_mut();
      plugins_scope_by_config.entry(key).or_default().clone()
    };
    // only allow one task in here per config
    let mut cell = cell.lock().await;
    let config = self.resolve_config(&config_file_bytes).await?;

    if let Some(cached_scope) = cell.as_ref() {
      let is_same_config = cached_scope.scope.config.as_deref() == Some(&config);
      if is_same_config && cached_scope.plugins_generation == self.plugins_generation.get() {
        return Ok(Some(cached_scope.scope.clone()));
      }
      // forget the scope so that failing to resolve the new one
      // doesn't have this be seen as a config change again
      cell.take();
      if !is_same_config {
        // For simplicity, shut down all plugins when any config changes in order
        // to do some cleanup. The scopes of the other configs are using those
        // plugins too, so this has them be resolved again when they're next used.
        self.plugins_generation.set(self.plugins_generation.get() + 1);
        self.plugin_resolver.clear_and_shutdown_initialized().await;
      }
    }

    // get this before resolving in case the plugins are shut down while resolving
    let plugins_generation = self.plugins_generation.get();
    let new_scope = Rc::new(resolve_plugins_scope(Rc::new(config), &self.environment, &self.plugin_resolver).await?);
    let _ = cell.insert(CachedScope {
      scope: new_scope.clone(),
      plugins_generation,
    });
    Ok(Some(new_scope))
  }

  /// Resolves the config of a config file, merging in the config file of an
  /// ancestor directory when it specifies `"inherit": true`.
  async fn resolve_config(&self, config_file: &ResolvedConfigPathWithText) -> Result<ResolvedConfig> {
    Ok(resolve_config_from_path_with_ancestors(config_file, self.remote_cache_mode, &self.environment).await?)
  }
}
