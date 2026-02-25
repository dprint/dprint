use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;

use crate::configuration::ResolvedConfigPath;
use crate::configuration::get_default_config_file_in_ancestor_directories;
use crate::configuration::resolve_config_from_path;
use crate::configuration::resolve_global_config_path;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins;
use crate::resolution::PluginsScope;
use crate::resolution::resolve_plugins_scope;
use crate::utils::AsyncMutex;
use crate::utils::PathSource;
use crate::utils::resolve_url_or_file_path;

type ScopeCell<TEnvironment> = AsyncMutex<Option<Rc<PluginsScope<TEnvironment>>>>;

pub struct LspPluginsScopeContainer<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>,
  plugins_scope_by_config: RefCell<HashMap<CanonicalizedPathBuf, Rc<ScopeCell<TEnvironment>>>>,
  // Config path override from --config CLI flag.
  config_override: Option<String>,
}

impl<TEnvironment: Environment> LspPluginsScopeContainer<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_resolver: Rc<plugins::PluginResolver<TEnvironment>>, config_override: Option<String>) -> Self {
    Self {
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

  async fn resolve_config_path_override(&self) -> Result<Option<ResolvedConfigPath>> {
    let Some(config) = &self.config_override else {
      return Ok(None);
    };
    let base_path = self.environment.cwd();
    let resolved_path = resolve_url_or_file_path(config, &PathSource::new_local(base_path.clone()), &self.environment).await?;
    Ok(Some(ResolvedConfigPath {
      resolved_path,
      base_path,
      is_global_config: false,
    }))
  }

  pub async fn resolve_by_path(&self, dir_path: &Path) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let config_path = if self.config_override.is_some() {
      self.resolve_config_path_override().await?
    } else {
      get_default_config_file_in_ancestor_directories(&self.environment, dir_path)?.or_else(|| resolve_global_config_path(&self.environment))
    };
    let Some(config_path) = config_path else {
      return Ok(None);
    };
    let cell = {
      let mut plugins_scope_by_config = self.plugins_scope_by_config.borrow_mut();
      plugins_scope_by_config.entry(config_path.resolved_path.file_path.clone()).or_default().clone()
    };
    // only allow one task in here per config
    let mut cell = cell.lock().await;
    let config = resolve_config_from_path(&config_path, &self.environment).await?;

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
