use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::PluginInfo;
use crate::types::ErrBox;
use serde::Serialize;
use std::path::Path;

/// Trait for implementing a Wasm or process plugin.
pub trait PluginHandler<TConfiguration: Clone + Serialize> {
  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
  /// Gets the plugin's plugin info.
  fn get_plugin_info(&mut self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn get_license_text(&mut self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format_text(
    &mut self,
    file_path: &Path,
    file_text: &str,
    config: &TConfiguration,
    format_with_host: impl FnMut(&Path, String, &ConfigKeyMap) -> Result<String, ErrBox>,
  ) -> Result<String, ErrBox>;
}
