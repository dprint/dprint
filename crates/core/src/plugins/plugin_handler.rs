use anyhow::Result;
use serde::Serialize;
use std::future::Future;
use std::ops::Range;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::PluginInfo;

pub trait CancellationToken: Send + Sync {
  fn is_cancelled(&self) -> bool;
}

pub trait Host: Send + Sync {
  fn format(
    &self,
    file_path: PathBuf,
    file_text: String,
    range: Option<Range<usize>>,
    config: Option<&ConfigKeyMap>,
  ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>>;
}

pub struct FormatRequest<TConfiguration, CancellationToken> {
  pub file_path: PathBuf,
  pub file_text: String,
  pub config: Arc<TConfiguration>,
  /// Range to format.
  pub range: Option<Range<usize>>,
  pub token: CancellationToken,
}

/// Trait for implementing a Wasm or process plugin.
pub trait PluginHandler: Send + Sync + 'static {
  type Configuration: Serialize + Clone + Send + Sync;

  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> ResolveConfigurationResult<Self::Configuration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format<TCancellationToken: CancellationToken>(
    &self,
    request: FormatRequest<Self::Configuration, TCancellationToken>,
    host: impl Host,
  ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>>;
}
