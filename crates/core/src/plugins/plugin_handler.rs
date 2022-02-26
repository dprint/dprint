use anyhow::Result;
use serde::Serialize;
use std::future::Future;
use std::path::Path;

use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::PluginInfo;

pub trait Host {
  type FormatFuture: Future<Output = Result<Option<String>>>;

  fn format(&self, file_path: &Path, file_text: &str, config: &ConfigKeyMap) -> Self::FormatFuture;
}

/// Trait for implementing a Wasm or process plugin.
pub trait PluginHandler {
  type Configuration: Serialize + Clone;
  type FormatFuture: Future<Output = Result<Option<String>>>;

  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&self, global_config: &GlobalConfiguration, config: ConfigKeyMap) -> ResolveConfigurationResult<Self::Configuration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(&self, file_path: &Path, file_text: &str, config: &Self::Configuration, host: &impl Host) -> Self::FormatFuture;
}
