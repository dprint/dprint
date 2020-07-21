use crate::utils::PathSource;
use bytes::Bytes;
use dprint_core::plugins::PluginInfo;
use std::path::PathBuf;

use crate::environment::Environment;
use crate::types::ErrBox;

// todo: Rename this file

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
