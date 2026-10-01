use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Context;
use anyhow::Result;

use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::get_default_config_file_in_ancestor_directories;
use crate::configuration::resolve_config_from_path_with_bytes;
use crate::configuration::resolve_global_config_path_and_text;
use crate::environment::Environment;
use crate::plugins;
use crate::resolution::PluginsScope;
use crate::resolution::resolve_plugins_scope;
use crate::utils::AsyncMutex;
use crate::utils::PathSource;

type ScopeCell<TEnvironment> = AsyncMutex<Option<Rc<PluginsScope<TEnvironment>>>>;

/// The directory whose files are formatted using a config file.
pub enum ConfigDir {
  /// The directory of the config file.
  Dir(PathBuf),
  /// The config file isn't for a certain directory (ex. the global config file).
  Any,
}

pub struct LspPluginsScopeContainer<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
  plugins_scope_by_config: RefCell<HashMap<String, Rc<ScopeCell<TEnvironment>>>>,
  config_override: Option<PathBuf>,
  use_global_config: bool,
}

impl<TEnvironment: Environment> LspPluginsScopeContainer<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>, config_override: Option<PathBuf>) -> Self {
    Self {
      use_global_config: use_global_config(&environment),
      environment,
      plugin_resolver,
      plugins_scope_by_config: Default::default(),
      config_override,
    }
  }

  pub async fn shutdown(&self) {
    self.plugins_scope_by_config.borrow_mut().clear();
    self.plugin_resolver.clear_and_shutdown_initialized().await;
  }

  /// Gets the directory whose files are formatted using the config
  /// file that the files in the provided directory are formatted with.
  pub fn resolve_config_dir(&self, dir_path: &Path) -> Result<Option<ConfigDir>> {
    Ok(self.resolve_config_file(dir_path, false)?.map(|config_file| {
      if self.config_override.is_some() || config_file.is_global_config {
        ConfigDir::Any
      } else {
        ConfigDir::Dir(config_file.base_path.into_path_buf())
      }
    }))
  }

  /// Resolves the plugins to format the files in the provided directory with.
  /// `use_global_config` is for using the global config file when there's no
  /// config file in an ancestor directory even when that's not done by default.
  pub async fn resolve_by_path(&self, dir_path: &Path, use_global_config: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let config_file_bytes = self.resolve_config_file(dir_path, use_global_config)?;
    let Some(config_file_bytes) = config_file_bytes else {
      return Ok(None);
    };
    let cell = {
      let mut plugins_scope_by_config = self.plugins_scope_by_config.borrow_mut();
      plugins_scope_by_config.entry(config_file_bytes.source.display()).or_default().clone()
    };
    // only allow one task in here per config
    let mut cell = cell.lock().await;
    let config = resolve_config_from_path_with_bytes(&config_file_bytes, &self.environment).await?;

    if let Some(existing_scope) = cell.as_ref() {
      if existing_scope.config.as_deref() == Some(&config) {
        return Ok(Some(existing_scope.clone()));
      }
      // for simplicity, shut down all plugins when any config
      // changes in order to do some cleanup
      self.plugin_resolver.clear_and_shutdown_initialized().await;
    }

    let new_scope = Rc::new(resolve_plugins_scope(Rc::new(config), &self.environment, &self.plugin_resolver).await?);
    let _ = cell.insert(new_scope.clone());
    Ok(Some(new_scope))
  }

  fn resolve_config_file(&self, dir_path: &Path, use_global_config: bool) -> Result<Option<ResolvedConfigPathWithText>> {
    Ok(if let Some(path) = &self.config_override {
      let path = self.environment.canonicalize(path).context("failed resolving --config path")?;
      let content = self.environment.read_file(&path).context("failed resolving --config path")?;
      Some(ResolvedConfigPathWithText {
        base_path: path.parent().unwrap_or_else(|| path.clone()),
        source: PathSource::new_local(path),
        is_first_download: false,
        content,
        is_global_config: false,
      })
    } else {
      match get_default_config_file_in_ancestor_directories(&self.environment, dir_path)? {
        Some(config) => Some(config),
        None if self.use_global_config || use_global_config => resolve_global_config_path_and_text(&self.environment)?,
        None => None,
      }
    })
  }
}

/// Whether to format the files that don't have a config file in an ancestor
/// directory using the global config file, which an editor may opt out of by
/// setting the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable to `0`
/// or `false` (ex. to only use it when the user enables that in the editor).
fn use_global_config(environment: &impl Environment) -> bool {
  !environment.env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG").is_some_and(|value| {
    let value = value.to_string_lossy();
    let value = value.trim();
    value == "0" || value.eq_ignore_ascii_case("false")
  })
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn use_global_config_env_var() {
    let environment = crate::environment::TestEnvironment::new();
    assert!(use_global_config(&environment));
    for (value, expected) in [
      ("0", false),
      ("false", false),
      ("FALSE", false),
      (
        " 0
", false,
      ),
      ("1", true),
      ("true", true),
      ("", true),
    ] {
      environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some(value));
      assert_eq!(use_global_config(&environment), expected, "{:?}", value);
    }
  }
}
