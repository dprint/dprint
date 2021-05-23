use crate::configuration::{ConfigKeyMap, GlobalConfiguration, ResolveConfigurationResult};
use std::path::Path;
use serde::Serialize;

/// Trait for implementing a Wasm plugin. Provide this to the generate_plugin_code macro.
pub trait WasmPlugin<TConfiguration> where TConfiguration : Clone + Serialize {
    /// Resolves configuration based on the provided config map and global configuration.
    fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
    /// Gets the plugin's configuration key (ex. "json").
    fn get_plugin_config_key(&mut self) -> String;
    /// Gets the file extensions the plugin supports. (ex. ["json"])
    fn get_plugin_file_extensions(&mut self) -> Vec<String>;
    /// Gets the plugin help url (ex. https://dprint.dev/plugins/json).
    fn get_plugin_help_url(&mut self) -> String;
    /// Gets the plugin's configuration schema url. Just return an empty string for now.
    fn get_plugin_config_schema_url(&mut self) -> String;
    /// Gets the plugin's license text.
    fn get_plugin_license_text(&mut self) -> String;
    /// Formats the provided file text based on the provided file path and configuration.
    fn format_text(&mut self, file_path: &Path, file_text: &str, config: &TConfiguration) -> Result<String, String>;
}
