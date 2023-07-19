use anyhow::Result;
use serde::Serialize;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::PluginInfo;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type FormatRange = Option<std::ops::Range<usize>>;

/// A formatting error where the plugin cannot recover.
///
/// Return one of these to signal to the dprint CLI that
/// it should recreate the plugin.
#[derive(Debug)]
pub struct CriticalFormatError(pub anyhow::Error);

impl std::fmt::Display for CriticalFormatError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.0.fmt(f)
  }
}

impl std::error::Error for CriticalFormatError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    self.0.source()
  }
}

pub trait CancellationToken: Send + Sync {
  fn is_cancelled(&self) -> bool;
  fn wait_cancellation(&self) -> BoxFuture<'static, ()>;
}

/// A cancellation token that always says it's not cancelled.
pub struct NullCancellationToken;

impl CancellationToken for NullCancellationToken {
  fn is_cancelled(&self) -> bool {
    false
  }

  fn wait_cancellation(&self) -> BoxFuture<'static, ()> {
    // never resolves
    Box::pin(std::future::pending())
  }
}

pub struct HostFormatRequest {
  pub file_path: PathBuf,
  pub file_text: String,
  /// Range to format.
  pub range: FormatRange,
  pub config_id: FormatConfigId,
  pub override_config: ConfigKeyMap,
  pub token: Arc<dyn CancellationToken>,
}

pub trait Host: Send + Sync {
  fn format(&self, request: HostFormatRequest) -> BoxFuture<FormatResult>;
}

/// Implementation of Host that always returns that
/// it can't format something.
pub struct NoopHost;

impl Host for NoopHost {
  fn format(&self, _: HostFormatRequest) -> BoxFuture<FormatResult> {
    Box::pin(async { Ok(None) })
  }
}

/// `Ok(Some(text))` - Changes due to the format.
/// `Ok(None)` - No changes.
/// `Err(err)` - Error formatting. Use a `CriticalError` to signal that the plugin can't recover.
pub type FormatResult = Result<Option<String>>;

/// A unique configuration id used for formatting.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct FormatConfigId(u32);

impl std::fmt::Display for FormatConfigId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "${}", self.0)
  }
}

impl FormatConfigId {
  pub fn from_raw(raw: u32) -> FormatConfigId {
    FormatConfigId(raw)
  }

  pub fn uninitialized() -> FormatConfigId {
    FormatConfigId(0)
  }

  pub fn as_raw(&self) -> u32 {
    self.0
  }
}

pub struct FormatRequest<TConfiguration> {
  pub file_path: PathBuf,
  pub file_text: String,
  pub config_id: FormatConfigId,
  pub config: Arc<TConfiguration>,
  /// Range to format.
  pub range: FormatRange,
  pub token: Arc<dyn CancellationToken>,
}

/// Trait for implementing a process plugin. Wasm plugins will eventually be changed to implement this.
pub trait AsyncPluginHandler: Send + Sync + 'static {
  type Configuration: Serialize + Clone + Send + Sync;

  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> ResolveConfigurationResult<Self::Configuration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(&self, request: FormatRequest<Self::Configuration>, host: Arc<dyn Host>) -> BoxFuture<FormatResult>;
}

/// Trait for implementing a Wasm plugin. Eventually this will be combined with AsyncPluginHandler.
pub trait SyncPluginHandler<TConfiguration: Clone + Serialize> {
  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&mut self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&mut self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(
    &mut self,
    file_path: &Path,
    file_text: &str,
    config: &TConfiguration,
    format_with_host: impl FnMut(&Path, String, &ConfigKeyMap) -> FormatResult,
  ) -> FormatResult;
}
