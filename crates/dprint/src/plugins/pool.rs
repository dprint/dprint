use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use dprint_core::types::ErrBox;

use super::output_plugin_config_diagnostics;
use super::InitializedPlugin;
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
pub struct PluginsDropper<TEnvironment: Environment> {
  pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> Drop for PluginsDropper<TEnvironment> {
  fn drop(&mut self) {
    self.pools.drop_plugins();
  }
}

impl<TEnvironment: Environment> PluginsDropper<TEnvironment> {
  pub fn new(pools: Arc<PluginPools<TEnvironment>>) -> Self {
    PluginsDropper { pools }
  }
}

#[derive(Default)]
struct PluginNameResolutionMaps {
  extension_to_plugin_name_map: HashMap<String, String>,
  file_name_to_plugin_name_map: HashMap<String, String>,
  association_matchers: Vec<(String, GlobMatcher)>,
}

pub struct PluginPools<TEnvironment: Environment> {
  environment: TEnvironment,
  pools: Mutex<HashMap<String, Arc<InitializedPluginPool<TEnvironment>>>>,
  plugin_name_maps: RwLock<PluginNameResolutionMaps>,
  /// Plugins may format using other plugins. If so, they should have a locally
  /// owned plugin instance that will be created on demand.
  plugins_for_plugins: Mutex<HashMap<String, HashMap<String, Vec<Box<dyn InitializedPlugin>>>>>,
}

impl<TEnvironment: Environment> PluginPools<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    PluginPools {
      environment,
      pools: Default::default(),
      plugin_name_maps: Default::default(),
      plugins_for_plugins: Default::default(),
    }
  }

  pub fn drop_plugins(&self) {
    {
      let mut pools = self.pools.lock();
      for pool in pools.values() {
        pool.drop_plugins();
      }
      pools.clear();
    }
    {
      let mut plugins_for_plugins = self.plugins_for_plugins.lock();
      plugins_for_plugins.clear();
    }
  }

  pub fn set_plugins(&self, plugins: Vec<Box<dyn Plugin>>, config_base_path: &CanonicalizedPathBuf) -> Result<(), ErrBox> {
    let mut pools = self.pools.lock();
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
          .or_insert(plugin_name.clone());
      }
      for file_name in plugin_file_names.iter() {
        // first added plugin takes precedence
        plugin_name_maps
          .file_name_to_plugin_name_map
          .entry(file_name.to_owned())
          .or_insert(plugin_name.clone());
      }

      if let Some(matchers) = get_plugin_association_glob_matcher(&plugin, &config_base_path)? {
        plugin_name_maps.association_matchers.push((plugin_name.clone(), matchers));
      }

      pools.insert(plugin_name.clone(), Arc::new(InitializedPluginPool::new(plugin, self.environment.clone())));
    }
    *self.plugin_name_maps.write() = plugin_name_maps;
    Ok(())
  }

  pub fn get_pool(&self, plugin_name: &str) -> Option<Arc<InitializedPluginPool<TEnvironment>>> {
    self.pools.lock().get(plugin_name).map(|p| p.clone())
  }

  pub fn take_instance_for_plugin(&self, parent_plugin_name: &str, sub_plugin_name: &str) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
    let plugin = self.with_plugins_for_parent_and_sub_plugin(parent_plugin_name, sub_plugin_name, |plugins| plugins.pop());

    if let Some(plugin) = plugin {
      Ok(plugin)
    } else {
      let pool = self.get_pool(sub_plugin_name).expect("Expected the plugin to exist in the pool.");
      if let Some(plugin) = pool.take_if_available() {
        Ok(plugin)
      } else {
        pool.create_instance()
      }
    }
  }

  pub fn release_instance_for_plugin(&self, parent_plugin_name: &str, sub_plugin_name: &str, plugin: Box<dyn InitializedPlugin>) {
    // There is a chance the data in plugins_for_plugins was already cleared by another thread.
    // If that occurs, ensure it is recreated to allow this plugin to be released into the
    // main pool once the `release` method is called by the worker.
    self.with_plugins_for_parent_and_sub_plugin(parent_plugin_name, sub_plugin_name, |plugins| {
      plugins.push(plugin);
    });
  }

  fn with_plugins_for_parent_and_sub_plugin<TResult>(
    &self,
    parent_plugin_name: &str,
    sub_plugin_name: &str,
    with_plugins: impl FnOnce(&mut Vec<Box<dyn InitializedPlugin>>) -> TResult,
  ) -> TResult {
    let mut plugins_for_plugins = self.plugins_for_plugins.lock();
    let plugins_for_plugin = if let Some(plugins_for_plugin) = plugins_for_plugins.get_mut(parent_plugin_name) {
      plugins_for_plugin
    } else {
      plugins_for_plugins.insert(parent_plugin_name.to_string(), HashMap::new());
      plugins_for_plugins.get_mut(parent_plugin_name).unwrap()
    };
    let mut plugins = if let Some(plugins) = plugins_for_plugin.get_mut(sub_plugin_name) {
      plugins
    } else {
      plugins_for_plugin.insert(sub_plugin_name.to_string(), Vec::new());
      plugins_for_plugin.get_mut(sub_plugin_name).unwrap()
    };

    with_plugins(&mut plugins)
  }

  pub fn get_plugin_name_from_file_name(&self, file_name: &Path) -> Option<String> {
    let plugin_name_maps = self.plugin_name_maps.read();
    get_lowercase_file_name(file_name)
      .map(|file_name| {
        plugin_name_maps
          .association_matchers
          .iter()
          .find(|(_, matcher)| matcher.is_match(&file_name))
          .map(|(plugin_name, _)| plugin_name)
          .or_else(|| plugin_name_maps.file_name_to_plugin_name_map.get(&file_name))
      })
      .flatten()
      .or_else(|| {
        get_lowercase_file_extension(file_name)
          .map(|ext| plugin_name_maps.extension_to_plugin_name_map.get(&ext))
          .flatten()
      })
      .map(|name| name.to_owned())
  }

  pub fn release(&self, parent_plugin_name: &str) {
    let plugins_for_plugin = self.plugins_for_plugins.lock().remove(parent_plugin_name);
    if let Some(plugins_for_plugin) = plugins_for_plugin {
      for (sub_plugin_name, initialized_plugins) in plugins_for_plugin.into_iter() {
        if let Some(pool) = self.get_pool(&sub_plugin_name) {
          pool.release_all(initialized_plugins);
        }
      }
    }
  }

  /// Gets a hash to be used for the "incremental" feature to tell if any plugins have changed.
  pub fn get_plugins_hash(&self) -> u64 {
    use std::num::Wrapping;
    // yeah, I know adding hashes isn't right, but the chance of this not working
    // in order to tell when a plugin has changed is super low.
    let pools = self.pools.lock();
    let mut hash_sum = Wrapping(0);
    for (_, pool) in pools.iter() {
      hash_sum += Wrapping(pool.plugin.get_hash());
    }
    hash_sum.0
  }
}

pub struct PoolTimeSnapshot {
  pub startup_time: u64,
  pub average_format_time: u64,
  pub has_plugin_available: bool,
}

struct PluginTimeStats {
  startup_time: u64,
  total_format_time: u64,
  format_count: u64,
}

pub enum TakePluginResult {
  HadDiagnostics,
  Success(Box<dyn InitializedPlugin>),
}

pub struct InitializedPluginPool<TEnvironment: Environment> {
  environment: TEnvironment,
  name: String,
  plugin: Box<dyn Plugin>,
  items: Mutex<Vec<Box<dyn InitializedPlugin>>>, // todo: RwLock
  time_stats: RwLock<PluginTimeStats>,
  checked_diagnostics: Mutex<Option<bool>>,
}

impl<TEnvironment: Environment> InitializedPluginPool<TEnvironment> {
  pub fn new(plugin: Box<dyn Plugin>, environment: TEnvironment) -> InitializedPluginPool<TEnvironment> {
    InitializedPluginPool {
      environment,
      name: plugin.name().to_string(),
      plugin: plugin,
      items: Mutex::new(Vec::new()),
      time_stats: RwLock::new(PluginTimeStats {
        // assume this if never created
        startup_time: 250,
        // give each plugin an average format time to start
        total_format_time: 50,
        format_count: 1,
      }),
      checked_diagnostics: Mutex::new(None),
    }
  }

  pub fn name(&self) -> &str {
    self.name.as_str()
  }

  pub fn drop_plugins(&self) {
    let mut items = self.items.lock();
    items.clear();
  }

  pub fn take_or_create_checking_config_diagnostics(&self, error_logger: &ErrorCountLogger<TEnvironment>) -> Result<TakePluginResult, ErrBox> {
    if let Some(plugin) = self.take_if_available() {
      Ok(TakePluginResult::Success(plugin))
    } else {
      let instance = self.create_instance()?;

      // only allow one thread to ever check and output the diagnostics (we don't want the messages being spammed)
      let mut has_checked_diagnostics = self.checked_diagnostics.lock();
      match *has_checked_diagnostics {
        Some(was_success) => {
          if !was_success {
            return Ok(TakePluginResult::HadDiagnostics);
          }
        }
        None => {
          let result = output_plugin_config_diagnostics(self.name(), &instance, &error_logger);
          *has_checked_diagnostics = Some(result.is_ok());
          if let Err(err) = result {
            self.environment.log_stderr(&err.to_string());
            return Ok(TakePluginResult::HadDiagnostics);
          }
        }
      }

      Ok(TakePluginResult::Success(instance))
    }
  }

  pub fn take_if_available(&self) -> Option<Box<dyn InitializedPlugin>> {
    let mut items = self.items.lock();
    items.pop()
  }

  pub fn release(&self, plugin: Box<dyn InitializedPlugin>) {
    let mut items = self.items.lock();
    items.push(plugin);
  }

  pub fn release_all(&self, plugins: Vec<Box<dyn InitializedPlugin>>) {
    let mut items = self.items.lock();
    items.extend(plugins);
  }

  pub fn get_time_snapshot(&self) -> PoolTimeSnapshot {
    let has_plugin_available = !self.items.lock().is_empty();
    let time_stats = self.time_stats.read();
    let average_format_time = (time_stats.total_format_time as f64 / time_stats.format_count as f64) as u64;
    PoolTimeSnapshot {
      startup_time: time_stats.startup_time,
      average_format_time,
      has_plugin_available,
    }
  }

  fn create_instance(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
    let start_instant = Instant::now();
    log_verbose!(self.environment, "Creating instance of {}", self.plugin.name());
    let plugin = self.plugin.initialize()?;
    let startup_duration = start_instant.elapsed().as_millis() as u64;
    log_verbose!(self.environment, "Created instance of {} in {}ms", self.plugin.name(), startup_duration);
    self.time_stats.write().startup_time = startup_duration; // store the latest duration
    Ok(plugin)
  }

  pub fn format_measuring_time<TResult>(&self, mut action: impl FnMut() -> TResult) -> TResult {
    let start_instant = Instant::now();
    let result = action();
    let elapsed_time = start_instant.elapsed();
    let mut time_stats = self.time_stats.write();
    time_stats.total_format_time += elapsed_time.as_millis() as u64;
    time_stats.format_count += 1;
    result
  }
}
