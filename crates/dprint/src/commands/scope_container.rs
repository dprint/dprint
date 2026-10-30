use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;

use crate::cache::RemoteCacheMode;
use crate::commands::process_plugin_approval::record_set_up_process_plugins;
use crate::configuration::Permissions;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::SessionPermissions;
use crate::configuration::get_default_config_file_in_ancestor_directories;
use crate::configuration::resolve_config_from_path_with_ancestors;
use crate::configuration::resolve_global_config_path_and_text_at_path_root;
use crate::configuration::resolve_local_config_file_dirs;
use crate::environment::Environment;
use crate::plugins;
use crate::resolution::PluginsScope;
use crate::resolution::resolve_plugins_scope_with_approval;
use crate::utils::AsyncMutex;
use crate::utils::PathSource;

type ScopeCell<TEnvironment> = AsyncMutex<Option<CachedScope<TEnvironment>>>;

struct CachedScope<TEnvironment: Environment> {
  scope: Rc<PluginsScope<TEnvironment>>,
  /// The generation of the plugins the scope was resolved with.
  plugins_generation: usize,
  /// What the user allowed when the scope was resolved, which decides the
  /// process plugins that are in it.
  permissions: Permissions,
}

/// Resolves and caches the plugins of each config file for the editors, which
/// format the files of a whole workspace and so find the config file of each
/// file (the language server and the editor service starting with schema
/// version 6).
pub struct PluginsScopeContainer<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
  plugins_scope_by_config: RefCell<HashMap<String, Rc<ScopeCell<TEnvironment>>>>,
  config_override: Option<PathBuf>,
  remote_cache_mode: RemoteCacheMode,
  /// Incremented each time the plugins are shut down, which is when the
  /// scopes that were resolved before then can't be used anymore.
  plugins_generation: Cell<usize>,
  /// What the user allowed for this session.
  session_permissions: Arc<SessionPermissions>,
}

impl<TEnvironment: Environment> PluginsScopeContainer<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
    config_override: Option<PathBuf>,
    remote_cache_mode: RemoteCacheMode,
    session_permissions: Arc<SessionPermissions>,
  ) -> Self {
    Self {
      environment,
      plugin_resolver,
      plugins_scope_by_config: Default::default(),
      config_override,
      remote_cache_mode,
      plugins_generation: Default::default(),
      session_permissions,
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
    match self.find_config_file(dir_path, use_global_config)? {
      Some(config_file) => Ok(Some(self.resolve_config_file(&config_file).await?)),
      None => Ok(None),
    }
  }

  /// Finds the config file to format the files in the provided directory with.
  pub fn find_config_file(&self, dir_path: &Path, use_global_config: bool) -> Result<Option<ResolvedConfigPathWithText>> {
    Ok(if let Some(path) = &self.config_override {
      let dirs = resolve_local_config_file_dirs(&self.environment, &self.environment.cwd().as_ref().join(path)).context("failed resolving --config path")?;
      let path = self.environment.canonicalize(path).context("failed resolving --config path")?;
      let content = self.environment.read_file(&path).context("failed resolving --config path")?;
      Some(ResolvedConfigPathWithText {
        base_path: dirs.base_path,
        anchored_patterns_base_path: dirs.anchored_patterns_base_path,
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
    })
  }

  /// Loads the config file at the path, which is `None` when it doesn't exist.
  pub fn load_config_file(&self, path: &Path) -> Result<Option<ResolvedConfigPathWithText>> {
    let Some(content) = self.environment.maybe_read_file(path)? else {
      return Ok(None);
    };
    let dirs = resolve_local_config_file_dirs(&self.environment, path)?;
    Ok(Some(ResolvedConfigPathWithText {
      base_path: dirs.base_path,
      anchored_patterns_base_path: dirs.anchored_patterns_base_path,
      source: PathSource::new_local(self.environment.canonicalize(path)?),
      is_first_download: false,
      content,
      is_global_config: false,
    }))
  }

  /// Resolves the plugins of the config file found by `find_config_file`.
  pub async fn resolve_config_file(&self, config_file: &ResolvedConfigPathWithText) -> Result<Rc<PluginsScope<TEnvironment>>> {
    let cell = {
      let mut plugins_scope_by_config = self.plugins_scope_by_config.borrow_mut();
      plugins_scope_by_config.entry(get_config_file_key(config_file)).or_default().clone()
    };
    // only allow one task in here per config
    let mut cell = cell.lock().await;
    let config = self.resolve_config(config_file).await?;
    // read each time so approving a process plugin or editing the file takes effect
    let permissions = self.session_permissions.read(&self.environment);

    if let Some(cached_scope) = cell.as_ref() {
      let is_same_config = cached_scope.scope.config.as_deref() == Some(&config);
      // a plugin may have been installed since
      let is_installed = cached_scope.scope.untrusted_downloads.is_empty();
      if is_same_config && is_installed && cached_scope.plugins_generation == self.plugins_generation.get() && cached_scope.permissions == permissions {
        return Ok(cached_scope.scope.clone());
      }
      // forget the scope so that failing to resolve the new one
      // doesn't have this be seen as a config change again
      cell.take();
    }

    // get this before resolving in case the plugins are shut down while resolving
    let plugins_generation = self.plugins_generation.get();
    // IMPORTANT: Process plugins don't run sandboxed, so the editors only use
    // the plugins when the user approved running its process plugins with their
    // configuration. The user opening a file (ex. in a cloned repository or in
    // node_modules) isn't the same as choosing to run dprint there.
    let config = Rc::new(config);
    let new_scope = Rc::new(resolve_plugins_scope_with_approval(config.clone(), &self.environment, &self.plugin_resolver, &permissions).await?);
    let permissions = if new_scope.plugins.is_empty() {
      permissions
    } else {
      // the configuration of a process plugin that the user allowed setting up is known now
      if let Err(err) = record_set_up_process_plugins(&self.environment, &self.session_permissions, &config, &self.plugin_resolver).await {
        log_warn!(self.environment, "Failed storing the decision for the process plugins: {:#}", err);
      }
      self.session_permissions.read(&self.environment)
    };
    let _ = cell.insert(CachedScope {
      scope: new_scope.clone(),
      plugins_generation,
      permissions,
    });
    drop(cell);
    self.shutdown_unused_plugins().await;
    Ok(new_scope)
  }

  /// Shuts down the plugins that none of the scopes use anymore (ex. a plugin
  /// that was removed from a config file), which leaves the others running so
  /// that changing a config file doesn't stop formatting with another.
  async fn shutdown_unused_plugins(&self) {
    let cells = self.plugins_scope_by_config.borrow().values().cloned().collect::<Vec<_>>();
    let mut used_plugins = Vec::new();
    for cell in cells {
      // a scope that's being resolved may use any of them
      let Some(cell) = cell.try_read() else {
        return;
      };
      if let Some(cached_scope) = cell.as_ref() {
        used_plugins.extend(cached_scope.scope.plugins.values().map(|plugin| plugin.plugin.clone()));
      }
    }
    self.plugin_resolver.shutdown_unused(&used_plugins).await;
  }

  /// Gets whether the server uses one config file for every file
  /// (`dprint lsp --config <path>`).
  pub fn has_config_override(&self) -> bool {
    self.config_override.is_some()
  }

  /// Gets the information of a plugin that was set up before, which doesn't
  /// set it up (ex. a process plugin is run to get its information).
  pub fn get_set_up_plugin_info(&self, plugin: &crate::plugins::PluginSourceReference) -> Option<dprint_core::plugins::PluginInfo> {
    self.plugin_resolver.get_set_up_plugin_info(plugin)
  }

  /// Gets the files a plugin matched the last time a config file was resolved
  /// with it, which doesn't set it up.
  pub fn get_last_file_matching_info(&self, plugin: &crate::plugins::PluginSourceReference) -> Option<dprint_core::plugins::FileMatchingInfo> {
    self.plugin_resolver.get_last_file_matching_info(plugin)
  }

  /// Remembers the files a plugin matched when a config file was resolved with it.
  pub async fn store_file_matching_info(
    &self,
    plugin: &crate::plugins::PluginSourceReference,
    file_matching: &dprint_core::plugins::FileMatchingInfo,
  ) -> Result<()> {
    self.plugin_resolver.store_file_matching_info(plugin, file_matching).await
  }

  /// Resolves the config of a config file, merging in the config file of an
  /// ancestor directory when it specifies `"inherit": true`.
  async fn resolve_config(&self, config_file: &ResolvedConfigPathWithText) -> Result<ResolvedConfig> {
    Ok(resolve_config_from_path_with_ancestors(config_file, self.remote_cache_mode, &self.environment).await?)
  }
}

/// Gets the key that identifies the config file found for a directory.
pub fn get_config_file_key(config_file: &ResolvedConfigPathWithText) -> String {
  // the base path is part of the key because the global config
  // file has a different one for each root directory (ex. drive)
  format!("{}\n{}", config_file.source.display(), config_file.base_path.display())
}
