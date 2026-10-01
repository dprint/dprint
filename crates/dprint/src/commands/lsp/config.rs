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

/// When to format the files that don't have a config file in an ancestor
/// directory using the global config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalConfigMode {
  Always,
  /// Only when the file isn't being formatted because it's being saved, so
  /// that saving a file in a project that doesn't use dprint doesn't format it.
  Explicit,
  Never,
}

impl GlobalConfigMode {
  /// An editor specifies this by setting the `DPRINT_EDITOR_USE_GLOBAL_CONFIG`
  /// environment variable to `explicit` or to `0` or `false` for never.
  pub fn from_env(environment: &impl Environment) -> Self {
    let Some(value) = environment.env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG") else {
      return Self::Always;
    };
    let value = value.to_string_lossy();
    let value = value.trim();
    if value.eq_ignore_ascii_case("explicit") {
      Self::Explicit
    } else if value == "0" || value.eq_ignore_ascii_case("false") {
      Self::Never
    } else {
      Self::Always
    }
  }

  fn allows(&self, is_save: bool) -> bool {
    match self {
      Self::Always => true,
      Self::Explicit => !is_save,
      Self::Never => false,
    }
  }
}

pub struct LspPluginsScopeContainer<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
  plugins_scope_by_config: RefCell<HashMap<String, Rc<ScopeCell<TEnvironment>>>>,
  config_override: Option<PathBuf>,
  global_config_mode: GlobalConfigMode,
}

impl<TEnvironment: Environment> LspPluginsScopeContainer<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>, config_override: Option<PathBuf>) -> Self {
    Self {
      global_config_mode: GlobalConfigMode::from_env(&environment),
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

  /// Resolves the plugins to format the files in the provided directory with. `is_save`
  /// is whether this is for formatting a file because it's being saved.
  pub async fn resolve_by_path(&self, dir_path: &Path, is_save: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let config_file_bytes = if let Some(path) = &self.config_override {
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
        None if self.global_config_mode.allows(is_save) => resolve_global_config_path_and_text(&self.environment)?,
        None => None,
      }
    };
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
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn global_config_mode_from_env() {
    let environment = crate::environment::TestEnvironment::new();
    assert_eq!(GlobalConfigMode::from_env(&environment), GlobalConfigMode::Always);
    for (value, expected) in [
      ("0", GlobalConfigMode::Never),
      ("false", GlobalConfigMode::Never),
      ("FALSE", GlobalConfigMode::Never),
      (" 0\n", GlobalConfigMode::Never),
      ("explicit", GlobalConfigMode::Explicit),
      ("Explicit", GlobalConfigMode::Explicit),
      ("1", GlobalConfigMode::Always),
      ("true", GlobalConfigMode::Always),
      ("", GlobalConfigMode::Always),
    ] {
      environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some(value));
      assert_eq!(GlobalConfigMode::from_env(&environment), expected, "{:?}", value);
    }
  }

  #[test]
  fn global_config_mode_allows() {
    assert!(GlobalConfigMode::Always.allows(true));
    assert!(GlobalConfigMode::Explicit.allows(false));
    assert!(!GlobalConfigMode::Explicit.allows(true));
    assert!(!GlobalConfigMode::Never.allows(false));
  }
}
