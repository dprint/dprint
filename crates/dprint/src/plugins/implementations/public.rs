use bytes::Bytes;
use dprint_core::plugins::PluginInfo;
use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::{Plugin, PluginSourceReference, PluginCache, PluginPools};
use crate::types::ErrBox;
use crate::utils::PathSource;
use super::process::{self};
use super::wasm::{self};

pub struct SetupPluginResult {
    pub file_path: PathBuf,
    pub plugin_info: PluginInfo,
}

pub async fn setup_plugin<TEnvironment: Environment>(
    url_or_file_path: &PathSource,
    file_bytes: &Bytes,
    environment: &TEnvironment
) -> Result<SetupPluginResult, ErrBox> {
    if url_or_file_path.is_wasm_plugin() {
        wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment).await
    } else if url_or_file_path.is_process_plugin() {
        process::setup_process_plugin(url_or_file_path, file_bytes, environment).await
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
}

pub fn get_file_path_from_plugin_info<TEnvironment: Environment>(
    url_or_file_path: &PathSource,
    plugin_info: &PluginInfo,
    environment: &TEnvironment,
) -> Result<PathBuf, ErrBox> {
    if url_or_file_path.is_wasm_plugin() {
        wasm::get_file_path_from_plugin_info(plugin_info, environment)
    } else if url_or_file_path.is_process_plugin() {
        process::get_file_path_from_plugin_info(plugin_info, environment)
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
}

/// Deletes the plugin from the cache.
pub fn cleanup_plugin<TEnvironment: Environment>(
    url_or_file_path: &PathSource,
    plugin_info: &PluginInfo,
    environment: &TEnvironment,
) -> Result<(), ErrBox> {
    if url_or_file_path.is_wasm_plugin() {
        wasm::cleanup_wasm_plugin(plugin_info, environment)
    } else if url_or_file_path.is_process_plugin() {
        process::cleanup_process_plugin(plugin_info, environment)
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
}

pub async fn create_plugin<TEnvironment : Environment>(
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    plugin_cache: &PluginCache<TEnvironment>,
    environment: TEnvironment,
    plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>, ErrBox> {
    let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await;
    let cache_item = match cache_item {
        Ok(cache_item) => Ok(cache_item),
        Err(err) => {
            environment.log_error(&format!(
                "Error getting plugin from cache. Forgetting from cache and retrying. Message: {}",
                err.to_string()
            ));

            // forget and try again
            plugin_cache.forget(plugin_reference)?;
            plugin_cache.get_plugin_cache_item(plugin_reference).await
        }
    }?;

    if plugin_reference.is_wasm_plugin() {
        let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
                environment.log_error(&format!(
                    "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {}",
                    err.to_string()
                ));

                // forget and try again
                plugin_cache.forget(plugin_reference)?;
                let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await?;
                environment.read_file_bytes(&cache_item.file_path)?
            }
        };

        Ok(Box::new(wasm::WasmPlugin::new(file_bytes, cache_item.info, plugin_pools)))
    } else if plugin_reference.is_process_plugin() {
        let cache_item = if !environment.path_exists(&cache_item.file_path) {
            environment.log_error(&format!(
                "Could not find process plugin at {}. Forgetting from cache and retrying.",
                cache_item.file_path.display()
            ));

            // forget and try again
            plugin_cache.forget(plugin_reference)?;
            plugin_cache.get_plugin_cache_item(plugin_reference).await?
        } else {
            cache_item
        };

        let executable_path = super::process::get_test_safe_executable_path(cache_item.file_path, &environment);
        Ok(Box::new(process::ProcessPlugin::new(executable_path, cache_item.info, plugin_pools)))
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
    }
}
