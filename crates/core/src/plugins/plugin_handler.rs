use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

#[cfg(feature = "async_runtime")]
use crate::async_runtime::FutureExt;
#[cfg(feature = "async_runtime")]
use crate::async_runtime::LocalBoxFuture;

use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigKeyValue;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::PluginInfo;

use super::FileMatchingInfo;

pub trait CancellationToken: Send + Sync + std::fmt::Debug {
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
#[derive(Debug)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckConfigUpdatesMessage {
  /// dprint versions < 0.47 won't have this set
  #[serde(default)]
  pub old_version: Option<String>,
  pub config: ConfigKeyMap,
}

#[cfg(feature = "process")]
#[derive(Debug)]
pub struct HostFormatRequest {
  pub file_path: std::path::PathBuf,
  pub file_bytes: Vec<u8>,
  /// Range to format.
  pub range: FormatRange,
  pub override_config: ConfigKeyMap,
  pub token: std::sync::Arc<dyn CancellationToken>,
}

#[cfg(feature = "wasm")]
#[derive(Debug)]
pub struct SyncHostFormatRequest<'a> {
  pub file_path: &'a std::path::Path,
  pub file_bytes: &'a [u8],
  /// Range to format.
  pub range: FormatRange,
  pub override_config: &'a ConfigKeyMap,
}

/// `Ok(Some(text))` - Changes due to the format.
/// `Ok(None)` - No changes.
/// `Err(err)` - Error formatting. Use a `CriticalError` to signal that the plugin can't recover.
pub type FormatResult = Result<Option<Vec<u8>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawFormatConfig {
  pub plugin: ConfigKeyMap,
  pub global: GlobalConfiguration,
}

/// A unique configuration id used for formatting.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[cfg(feature = "process")]
pub struct FormatRequest<TConfiguration> {
  pub file_path: std::path::PathBuf,
  pub file_bytes: Vec<u8>,
  pub config_id: FormatConfigId,
  pub config: std::sync::Arc<TConfiguration>,
  /// Range to format.
  pub range: FormatRange,
  pub token: std::sync::Arc<dyn CancellationToken>,
}

#[cfg(feature = "wasm")]
pub struct SyncFormatRequest<'a, TConfiguration> {
  pub file_path: &'a std::path::Path,
  pub file_bytes: Vec<u8>,
  pub config_id: FormatConfigId,
  pub config: &'a TConfiguration,
  /// Range to format.
  pub range: FormatRange,
  pub token: &'a dyn CancellationToken,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigChangePathItem {
  /// String property name.
  String(String),
  /// Number if an index in an array.
  Number(usize),
}

impl From<String> for ConfigChangePathItem {
  fn from(value: String) -> Self {
    Self::String(value)
  }
}

impl From<usize> for ConfigChangePathItem {
  fn from(value: usize) -> Self {
    Self::Number(value)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChange {
  /// The path to make modifications at.
  pub path: Vec<ConfigChangePathItem>,
  #[serde(flatten)]
  pub kind: ConfigChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ConfigChangeKind {
  /// Adds an object property or array element.
  Add(ConfigKeyValue),
  /// Overwrites an existing value at the provided path.
  Set(ConfigKeyValue),
  /// Removes the value at the path.
  Remove,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResolveConfigurationResult<T>
where
  T: Clone + Serialize,
{
  /// Information about what files are matched for the provided configuration.
  pub file_matching: FileMatchingInfo,

  /// The configuration diagnostics.
  pub diagnostics: Vec<ConfigurationDiagnostic>,

  /// The configuration derived from the unresolved configuration
  /// that can be used to format a file.
  pub config: T,
}

/// Trait for implementing a process plugin.
#[cfg(feature = "process")]
#[crate::async_runtime::async_trait(?Send)]
pub trait AsyncPluginHandler: 'static {
  type Configuration: Serialize + Clone + Send + Sync;

  /// Gets the plugin's plugin info.
  fn plugin_info(&self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&self) -> String;
  /// Resolves configuration based on the provided config map and global configuration.
  async fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> PluginResolveConfigurationResult<Self::Configuration>;
  /// Updates the config key map. This will be called after the CLI has upgraded the
  /// plugin in `dprint config update`.
  async fn check_config_updates(&self, _message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    Ok(Vec::new())
  }
  /// Formats the provided file text based on the provided file path and configuration.
  async fn format(
    &self,
    request: FormatRequest<Self::Configuration>,
    format_with_host: impl FnMut(HostFormatRequest) -> LocalBoxFuture<'static, FormatResult> + 'static,
  ) -> FormatResult;
}

/// Trait for implementing a Wasm plugin.
#[cfg(feature = "wasm")]
pub trait SyncPluginHandler<TConfiguration: Clone + serde::Serialize> {
  /// Resolves configuration based on the provided config map and global configuration.
  fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> PluginResolveConfigurationResult<TConfiguration>;
  /// Gets the plugin's plugin info.
  fn plugin_info(&mut self) -> PluginInfo;
  /// Gets the plugin's license text.
  fn license_text(&mut self) -> String;
  /// Updates the config key map. This will be called after the CLI has upgraded the
  /// plugin in `dprint config update`.
  fn check_config_updates(&self, message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>>;
  /// Formats the provided file text based on the provided file path and configuration.
  fn format(&mut self, request: SyncFormatRequest<TConfiguration>, format_with_host: impl FnMut(SyncHostFormatRequest) -> FormatResult) -> FormatResult;
}
