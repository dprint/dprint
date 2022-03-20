use anyhow::bail;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::Host;
use dprint_core::plugins::HostFormatRequest;
use futures::FutureExt;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use super::output_plugin_config_diagnostics;
use super::InitializedPlugin;
use super::InitializedPluginFormatRequest;
use super::Plugin;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_plugin_association_glob_matcher;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;
use crate::utils::ErrorCountLogger;
use crate::utils::GlobMatcher;

/// This is necessary because of a circular reference where
/// PluginPools hold plugins and the plugins hold a PluginPools.
pub struct PluginsDropper<TEnvironment: Environment>(Arc<PluginsCollection<TEnvironment>>);

impl<TEnvironment: Environment> Drop for PluginsDropper<TEnvironment> {
  fn drop(&mut self) {
    self.0.drop_plugins();
  }
}

impl<TEnvironment: Environment> PluginsDropper<TEnvironment> {
  pub fn new(pools: Arc<PluginsCollection<TEnvironment>>) -> Self {
    Self(pools)
  }
}

#[derive(Default)]
struct PluginNameResolutionMaps {
  extension_to_plugin_name_map: HashMap<String, String>,
  file_name_to_plugin_name_map: HashMap<String, String>,
  association_matchers: Vec<(String, GlobMatcher)>,
}

pub struct PluginsCollection<TEnvironment: Environment> {
  environment: TEnvironment,
  plugins: Mutex<HashMap<String, Arc<PluginWrapper<TEnvironment>>>>,
  plugin_name_maps: RwLock<PluginNameResolutionMaps>,
}

impl<TEnvironment: Environment> PluginsCollection<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    Self {
      environment,
      plugins: Default::default(),
      plugin_name_maps: Default::default(),
    }
  }

  pub fn drop_plugins(&self) {
    self.plugins.lock().clear();
  }

  pub fn set_plugins(&self, plugins: Vec<Box<dyn Plugin>>, config_base_path: &CanonicalizedPathBuf) -> Result<()> {
    let mut self_plugins = self.plugins.lock();
    let mut plugin_name_maps: PluginNameResolutionMaps = Default::default();
    for plugin in plugins {
      let plugin_name = plugin.name().to_string();
      let plugin_extensions = plugin.file_extensions().clone();
      let plugin_file_names = plugin.file_names().clone();

      for extension in plugin_extensions.iter() {
        // first added plugin takes precedence
        plugin_name_maps
          .extension_to_plugin_name_map
          .entry(extension.to_owned())
          .or_insert_with(|| plugin_name.clone());
      }
      for file_name in plugin_file_names.iter() {
        // first added plugin takes precedence
        plugin_name_maps
          .file_name_to_plugin_name_map
          .entry(file_name.to_owned())
          .or_insert_with(|| plugin_name.clone());
      }

      if let Some(matchers) = get_plugin_association_glob_matcher(&*plugin, config_base_path)? {
        plugin_name_maps.association_matchers.push((plugin_name.clone(), matchers));
      }

      self_plugins.insert(plugin_name.clone(), Arc::new(PluginWrapper::new(plugin, self.environment.clone())));
    }
    *self.plugin_name_maps.write() = plugin_name_maps;
    Ok(())
  }

  pub fn get_plugin(&self, plugin_name: &str) -> Arc<PluginWrapper<TEnvironment>> {
    self
      .plugins
      .lock()
      .get(plugin_name)
      .cloned()
      .unwrap_or_else(|| panic!("Expected to find plugin in collection: {}", plugin_name))
  }

  pub fn get_plugin_names_from_file_name(&self, file_name: &Path) -> Vec<String> {
    let mut plugin_names = Vec::new();
    let plugin_name_maps = self.plugin_name_maps.read();

    if let Some(file_name) = get_lowercase_file_name(file_name) {
      for (plugin_name, matcher) in plugin_name_maps.association_matchers.iter() {
        if matcher.is_match(&file_name) {
          plugin_names.push(plugin_name.to_owned());
        }
      }
      if !plugin_names.is_empty() {
        return plugin_names;
      }

      if let Some(plugin_name) = plugin_name_maps.file_name_to_plugin_name_map.get(&file_name) {
        plugin_names.push(plugin_name.to_owned());
        return plugin_names;
      }
    }

    if let Some(ext) = get_lowercase_file_extension(file_name) {
      if let Some(plugin_name) = plugin_name_maps.extension_to_plugin_name_map.get(&ext) {
        plugin_names.push(plugin_name.to_owned());
      }
    }

    plugin_names
  }

  /// Gets a hash to be used for the "incremental" feature to tell if any plugins have changed.
  pub fn get_plugins_hash(&self) -> u64 {
    use std::num::Wrapping;
    // yeah, I know adding hashes isn't right, but the chance of this not working
    // in order to tell when a plugin has changed is super low.
    let plugins = self.plugins.lock();
    let mut hash_sum = Wrapping(0);
    for plugin in plugins.values() {
      hash_sum += Wrapping(plugin.plugin.get_hash());
    }
    hash_sum.0
  }
}

impl<TEnvironment: Environment> Host for PluginsCollection<TEnvironment> {
  fn format(&self, request: HostFormatRequest) -> dprint_core::plugins::BoxFuture<FormatResult> {
    let mut file_text = request.file_text;
    let plugin_names = self.get_plugin_names_from_file_name(&request.file_path);
    async move {
      let mut had_change = false;
      for plugin_name in plugin_names {
        let plugin = self.get_plugin(&plugin_name);
        let error_logger = ErrorCountLogger::from_environment(&self.environment);
        match plugin.get_or_create_checking_config_diagnostics(error_logger.clone()).await {
          Ok(GetPluginResult::Success(initialized_plugin)) => {
            let result = initialized_plugin
              .format_text(InitializedPluginFormatRequest {
                file_path: request.file_path.clone(),
                file_text: file_text.clone(),
                range: request.range.clone(),
                override_config: request.override_config.clone(),
                token: request.token.clone(),
              })
              .await;
            if let Some(new_text) = result? {
              file_text = new_text;
              had_change = true;
            }
          }
          Ok(GetPluginResult::HadDiagnostics) => bail!("Had {} configuration errors.", error_logger.get_error_count()),
          Err(err) => return Err(CriticalFormatError(err).into()),
        }
      }

      Ok(if had_change { Some(file_text) } else { None })
    }
    .boxed()
  }
}

pub enum GetPluginResult {
  HadDiagnostics,
  Success(Arc<dyn InitializedPlugin>),
}

pub struct PluginWrapper<TEnvironment: Environment> {
  environment: TEnvironment,
  name: String,
  plugin: Box<dyn Plugin>,
  initialized_plugin: tokio::sync::Mutex<Option<Arc<dyn InitializedPlugin>>>,
  checked_diagnostics: Mutex<Option<bool>>,
}

impl<TEnvironment: Environment> PluginWrapper<TEnvironment> {
  pub fn new(plugin: Box<dyn Plugin>, environment: TEnvironment) -> Self {
    Self {
      environment,
      name: plugin.name().to_string(),
      plugin,
      initialized_plugin: Default::default(),
      checked_diagnostics: Default::default(),
    }
  }

  pub fn name(&self) -> &str {
    self.name.as_str()
  }

  pub async fn get_or_create_checking_config_diagnostics(&self, error_logger: ErrorCountLogger<TEnvironment>) -> Result<GetPluginResult> {
    // only allow one thread to initialize and output the diagnostics (we don't want the messages being spammed)
    let mut initialized_plugin = self.initialized_plugin.lock().await;
    if let Some(plugin) = initialized_plugin.clone() {
      Ok(GetPluginResult::Success(plugin))
    } else {
      let instance = self.create_instance().await?;

      let has_checked_diagnostics = *self.checked_diagnostics.lock();
      match has_checked_diagnostics {
        Some(was_success) => {
          if !was_success {
            return Ok(GetPluginResult::HadDiagnostics);
          }
        }
        None => {
          let result = output_plugin_config_diagnostics(self.name(), instance.clone(), error_logger).await;
          *self.checked_diagnostics.lock() = Some(result.is_ok());
          if let Err(err) = result {
            self.environment.log_stderr(&err.to_string());
            return Ok(GetPluginResult::HadDiagnostics);
          }
        }
      }

      *initialized_plugin = Some(instance.clone());
      Ok(GetPluginResult::Success(instance))
    }
  }

  async fn create_instance(&self) -> Result<Arc<dyn InitializedPlugin>> {
    let start_instant = Instant::now();
    log_verbose!(self.environment, "Creating instance of {}", self.plugin.name());
    let plugin = self.plugin.initialize().await?;
    let startup_duration = start_instant.elapsed().as_millis() as u64;
    log_verbose!(self.environment, "Created instance of {} in {}ms", self.plugin.name(), startup_duration);
    Ok(plugin)
  }
}
