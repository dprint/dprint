use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "async_runtime")]
use crate::async_runtime::LocalBoxFuture;
#[cfg(feature = "async_runtime")]
use futures::FutureExt;

#[cfg(any(feature = "wasm", feature = "process"))]
use crate::configuration::ConfigKeyMap;
#[cfg(any(feature = "wasm", feature = "process"))]
use crate::configuration::GlobalConfiguration;
#[cfg(any(feature = "wasm", feature = "process"))]
use crate::configuration::ResolveConfigurationResult;
#[cfg(any(feature = "wasm", feature = "process"))]
use crate::plugins::PluginInfo;

pub trait CancellationToken: Send + Sync {
  fn is_cancelled(&self) -> bool;
  #[cfg(feature = "async_runtime")]
  fn wait_cancellation(&self) -> LocalBoxFuture<'static, ()>;
}

#[cfg(feature = "async_runtime")]
impl CancellationToken for tokio_util::sync::CancellationToken {
  fn is_cancelled(&self) -> bool {
    self.is_cancelled()
  }

  fn wait_cancellation(&self) -> LocalBoxFuture<'static, ()> {
    let token = self.clone();
    async move { token.cancelled().await }.boxed_local()
  }
}

/// A cancellation token that always says it's not cancelled.
pub struct NullCancellationToken;

impl CancellationToken for NullCancellationToken {
  fn is_cancelled(&self) -> bool {
    false
  }

  #[cfg(feature = "async_runtime")]
  fn wait_cancellation(&self) -> LocalBoxFuture<'static, ()> {
    // never resolves
    Box::pin(std::future::pending())
  }
}

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

#[cfg(feature = "process")]
pub struct HostFormatRequest {
  pub file_path: PathBuf,
  pub file_text: String,
  /// Range to format.
  pub range: FormatRange,
  pub override_config: ConfigKeyMap,
  pub token: Arc<dyn CancellationToken>,
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

/// Trait for implementing a process plugin.
#[cfg(feature = "process")]
pub trait AsyncPluginHandler: Send + Sync + 'static {
  type Configuration: serde::Serialize + Clone + Send + Sync;

  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> ResolveConfigurationResult<Self::Configuration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(
    &self,
    request: FormatRequest<Self::Configuration>,
    format_with_host: impl FnMut(HostFormatRequest) -> LocalBoxFuture<'static, FormatResult> + 'static,
  ) -> LocalBoxFuture<'static, FormatResult>;
}

/// Trait for implementing a Wasm plugin.
#[cfg(feature = "wasm")]
pub trait SyncPluginHandler<TConfiguration: Clone + serde::Serialize> {
  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&mut self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&mut self) -> String;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(
    &mut self,
    file_path: &std::path::Path,
    file_text: &str,
    config: &TConfiguration,
    format_with_host: impl FnMut(&std::path::Path, String, &ConfigKeyMap) -> FormatResult,
  ) -> FormatResult;
}
