use crate::plugins::{Plugin, PluginSourceReference, PluginCache};
use crate::plugins::process::ProcessPlugin;
use crate::plugins::wasm::{PoolImportObjectFactory, WasmPlugin};
use crate::utils::PathSource;
use bytes::Bytes;
use dprint_core::plugins::PluginInfo;
use std::path::PathBuf;

use crate::environment::Environment;
use crate::types::ErrBox;

pub struct PluginSetupResult {
    pub file_path: PathBuf,
    pub plugin_info: PluginInfo,
}

pub async fn setup_plugin<TEnvironment: Environment>(
    url_or_file_path: &PathSource,
    file_bytes: &Bytes,
    environment: &TEnvironment
) -> Result<PluginSetupResult, ErrBox> {
    if url_or_file_path.is_wasm_plugin() {
        crate::plugins::wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment).await
    } else if url_or_file_path.is_process_plugin() {
        crate::plugins::process::setup_process_plugin(url_or_file_path, file_bytes, environment).await
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
        crate::plugins::wasm::get_file_path_from_plugin_info(plugin_info, environment)
    } else if url_or_file_path.is_process_plugin() {
        crate::plugins::process::get_file_path_from_plugin_info(plugin_info, environment)
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
        crate::plugins::wasm::cleanup_wasm_plugin(plugin_info, environment)
    } else if url_or_file_path.is_process_plugin() {
        crate::plugins::process::cleanup_process_plugin(plugin_info, environment)
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
}

pub async fn create_plugin<TEnvironment : Environment>(
    import_object_factory: PoolImportObjectFactory<TEnvironment>,
    plugin_cache: &PluginCache<TEnvironment>,
    environment: TEnvironment,
    plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>, ErrBox> {
    let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await;
    let cache_item = match cache_item {
        Ok(cache_item) => Ok(cache_item),
        Err(err) => {
            environment.log_error(&format!(
                "Error getting plugin from cache. Forgetting from cache and retrying. Message: {:?}",
                err
            ));

            // forget and try again
            plugin_cache.forget(plugin_reference)?;
            plugin_cache.get_plugin_cache_item(plugin_reference).await
        }
    }?;

    // todo: consolidate with setup_plugin.rs so all code like this is in the same place
    if plugin_reference.is_wasm_plugin() {
        let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
                environment.log_error(&format!(
                    "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {:?}",
                    err
                ));

                // forget and try again
                plugin_cache.forget(plugin_reference)?;
                let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await?;
                environment.read_file_bytes(&cache_item.file_path)?
            }
        };

        Ok(Box::new(WasmPlugin::new(file_bytes, cache_item.info, import_object_factory)))
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

        Ok(Box::new(ProcessPlugin::new(cache_item.info, cache_item.file_path)))
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
    }
}
