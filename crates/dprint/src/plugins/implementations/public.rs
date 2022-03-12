use anyhow::bail;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use dprint_core::plugins::PluginInfo;

use super::process;
use super::wasm;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginsCollection;
use crate::utils::PathSource;

pub struct SetupPluginResult {
  pub file_path: PathBuf,
  pub plugin_info: PluginInfo,
}

pub fn setup_plugin<TEnvironment: Environment>(url_or_file_path: &PathSource, file_bytes: &[u8], environment: &TEnvironment) -> Result<SetupPluginResult> {
  if url_or_file_path.is_wasm_plugin() {
    wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment)
  } else if url_or_file_path.is_process_plugin() {
    process::setup_process_plugin(url_or_file_path, file_bytes, environment)
  } else {
    bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
  }
}

pub fn get_file_path_from_plugin_info<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_info: &PluginInfo,
  environment: &TEnvironment,
) -> Result<PathBuf> {
  if url_or_file_path.is_wasm_plugin() {
    Ok(wasm::get_file_path_from_plugin_info(plugin_info, environment))
  } else if url_or_file_path.is_process_plugin() {
    Ok(process::get_file_path_from_plugin_info(plugin_info, environment))
  } else {
    bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
  }
}

/// Deletes the plugin from the cache.
pub fn cleanup_plugin<TEnvironment: Environment>(url_or_file_path: &PathSource, plugin_info: &PluginInfo, environment: &TEnvironment) -> Result<()> {
  if url_or_file_path.is_wasm_plugin() {
    wasm::cleanup_wasm_plugin(plugin_info, environment)
  } else if url_or_file_path.is_process_plugin() {
    process::cleanup_process_plugin(plugin_info, environment)
  } else {
    bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
  }
}

pub fn create_plugin<TEnvironment: Environment>(
  plugin_pools: Arc<PluginsCollection<TEnvironment>>,
  plugin_cache: &PluginCache<TEnvironment>,
  environment: TEnvironment,
  plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>> {
  let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference);
  let cache_item = match cache_item {
    Ok(cache_item) => Ok(cache_item),
    Err(err) => {
      log_verbose!(
        environment,
        "Error getting plugin from cache. Forgetting from cache and retrying. Message: {}",
        err.to_string()
      );

      // forget and try again
      plugin_cache.forget(plugin_reference)?;
      plugin_cache.get_plugin_cache_item(plugin_reference)
    }
  }?;

  if plugin_reference.is_wasm_plugin() {
    let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
      Ok(file_bytes) => file_bytes,
      Err(err) => {
        log_verbose!(
          environment,
          "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {}",
          err.to_string()
        );

        // forget and try again
        plugin_cache.forget(plugin_reference)?;
        let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference)?;
        environment.read_file_bytes(&cache_item.file_path)?
      }
    };

    Ok(Box::new(wasm::WasmPlugin::new(file_bytes, cache_item.info, plugin_pools)?))
  } else if plugin_reference.is_process_plugin() {
    let cache_item = if !environment.path_exists(&cache_item.file_path) {
      log_verbose!(
        environment,
        "Could not find process plugin at {}. Forgetting from cache and retrying.",
        cache_item.file_path.display()
      );

      // forget and try again
      plugin_cache.forget(plugin_reference)?;
      plugin_cache.get_plugin_cache_item(plugin_reference)?
    } else {
      cache_item
    };

    let executable_path = super::process::get_test_safe_executable_path(cache_item.file_path, &environment);
    Ok(Box::new(process::ProcessPlugin::new(
      environment,
      executable_path,
      cache_item.info,
      plugin_pools,
    )))
  } else {
    bail!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
  }
}
