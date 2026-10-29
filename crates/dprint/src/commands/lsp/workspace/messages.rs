//! The params and results of the `dprint/*` methods of the language server,
//! which an editor extension uses instead of the text document methods (see
//! docs/editor-extension-development.md).

use deno_tower_lsp::lsp_types::Range;
use deno_tower_lsp::lsp_types::TextEdit;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::configuration::ConfigKeyMap;
use serde::Deserialize;
use serde::Serialize;

/// The name of the experimental capability that a client provides to use the
/// `dprint/*` methods and that the server provides when it supports them.
pub const EXPERIMENTAL_CAPABILITY_NAME: &str = "dprint";
/// The version of the `dprint/*` methods, which changes when they change in a
/// way that a client needs to know about.
pub const EXPERIMENTAL_CAPABILITY_VERSION: u32 = 1;

pub const FORMAT_METHOD: &str = "dprint/format";
pub const FILE_INFO_METHOD: &str = "dprint/fileInfo";
pub const CONFIG_INFO_METHOD: &str = "dprint/configInfo";
pub const CONFIG_SCHEMA_METHOD: &str = "dprint/configSchema";
pub const APPROVE_METHOD: &str = "dprint/approve";
pub const RESET_METHOD: &str = "dprint/reset";
pub const CONFIG_CHANGED_NOTIFICATION: &str = "dprint/configChanged";

/// The capability that the server provides in the experimental capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalCapability {
  pub version: u32,
}

/// What identifies a document, which is formatted as a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentParams {
  pub text_document: DocumentIdentifier,
  /// The language of the document, which is needed for a document that isn't
  /// a file (ex. an untitled document or a notebook cell).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub language: Option<DocumentLanguage>,
  /// The notebook when the document is one of its cells, which is only
  /// formatted when the notebook would be.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notebook_document: Option<DocumentIdentifier>,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory, which is in addition to the setting.
  #[serde(default)]
  pub use_global_config: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentIdentifier {
  pub uri: Uri,
}

/// The language of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLanguage {
  /// The editor's id of the language (ex. "typescript").
  pub id: String,
  /// The file extensions the editor associates with the language with the
  /// leading period (ex. ".ts").
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub file_extensions: Vec<String>,
  /// The file names the editor associates with the language (ex. "Dockerfile").
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub file_names: Vec<String>,
}

/// The params of `dprint/format`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatParams {
  #[serde(flatten)]
  pub document: DocumentParams,
  /// The text of the document, which the server doesn't otherwise have.
  pub text: String,
  /// The ranges of the text to format, which is the whole text when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ranges: Option<Vec<Range>>,
  /// The plugin config to use instead of what's in the config file.
  #[serde(default, skip_serializing_if = "ConfigKeyMap::is_empty")]
  pub override_config: ConfigKeyMap,
}

/// The result of `dprint/format`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormatResult {
  /// The edits of the text that was sent, which don't overlap and are in
  /// order. It's empty when the text is already formatted.
  #[serde(rename_all = "camelCase")]
  Formatted { edits: Vec<TextEdit> },
  /// Why the document wasn't formatted.
  #[serde(rename_all = "camelCase")]
  NotFormatted {
    reason: NotFormattedReason,
    /// The message to show the user about why it wasn't formatted (ex. for a
    /// command the user ran).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// What the user needs to approve to format it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approval: Option<PendingApproval>,
  },
  /// Why formatting the document failed.
  #[serde(rename_all = "camelCase")]
  Failed { reason: FormatFailedReason, message: String },
}

/// Why formatting a file failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatFailedReason {
  /// A plugin couldn't format the file, which is usually because it has a
  /// syntax error.
  FormatError,
  /// A plugin failed (ex. it panicked or couldn't be set up).
  PluginError,
}

/// Why a file isn't formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotFormattedReason {
  /// There's no config file in an ancestor directory and there's no global
  /// config file.
  NoConfigFile,
  /// There's no config file in an ancestor directory and the global config
  /// file isn't allowed to be used.
  GlobalConfigNotAllowed,
  /// The config file doesn't format the file (ex. its includes and excludes
  /// don't match it).
  NotMatched,
  /// No plugin in the config file formats the file.
  NoPlugin,
  /// A plugin that formats the file doesn't support formatting a range.
  RangeFormattingNotSupported,
  /// The configuration of a plugin has problems, which are in the
  /// diagnostics of the config file.
  ConfigDiagnostics,
  /// The config file failed to resolve.
  ConfigError,
  /// Something needs to be approved before the config file is used, which the
  /// approval says (ex. its kind is `download` or `processPlugins`).
  ApprovalRequired,
}

/// Something the user needs to approve before a config file is used, which a
/// client approves with `dprint/approve` after asking the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
  /// Identifies exactly what's approved, which changes when it does (ex. the
  /// configuration of a process plugin).
  pub id: String,
  pub kind: ApprovalKind,
  /// The config file that needs it, which is what the user approves for.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<Uri>,
  /// The urls or paths to download for `download`, or the process plugins to
  /// run for `processPlugins`.
  pub items: Vec<String>,
  /// The configuration of the process plugins that are set up by their config
  /// key, which is what they run with (ex. the commands of dprint-plugin-exec).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub configuration: Option<serde_json::Map<String, serde_json::Value>>,
  /// Whether the user denied it, which means they shouldn't be asked unless
  /// they ask for it (ex. a command).
  pub denied: bool,
  /// The message to show the user when asking.
  pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
  /// Downloading from a domain that isn't trusted.
  Download,
  /// Running process plugins, which aren't sandboxed.
  ProcessPlugins,
}

/// The params of `dprint/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveParams {
  pub id: String,
  pub decision: ApprovalDecision,
}

/// What the user decided, which is only remembered for the session unless it's
/// `alwaysAllow` or `alwaysDeny`. A download is only allowed for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
  Allow,
  AlwaysAllow,
  Deny,
  AlwaysDeny,
}

impl ApprovalDecision {
  pub fn is_allowed(self) -> bool {
    matches!(self, ApprovalDecision::Allow | ApprovalDecision::AlwaysAllow)
  }
}

/// The result of `dprint/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveResult {
  /// What needs to be approved now, which is `None` only when nothing does.
  /// It's what was denied when the decision was to deny it, and otherwise it's
  /// what else needs to be approved (ex. a config file that a downloaded one
  /// extends or a process plugin the user denied before).
  pub approval: Option<PendingApproval>,
}

/// The result of `dprint/fileInfo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoResult {
  /// The config file that's used for the file, which is `None` when there isn't one.
  pub config_file: Option<ConfigFileInfo>,
  pub can_format: bool,
  /// The plugins that format the file in the order they do, which is empty
  /// when it can't be formatted.
  pub plugins: Vec<FileInfoPlugin>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<NotFormattedReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approval: Option<PendingApproval>,
}

/// A plugin that formats a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoPlugin {
  pub name: String,
  pub version: String,
}

/// The params of `dprint/configInfo` and `dprint/configSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigParams {
  /// The config file or a document that it's used for.
  pub uri: Uri,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory, which is in addition to the setting.
  #[serde(default)]
  pub use_global_config: bool,
}

/// The result of `dprint/configInfo`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfoResult {
  pub config_file: Option<ConfigFileInfo>,
  /// The plugins of the config file, which is `None` when they aren't known
  /// (ex. they need to be approved and weren't set up before).
  pub plugins: Option<Vec<PluginInfo>>,
  /// Whether the config file formats files other than the ones with the file
  /// extensions and names of its plugins (ex. with `associations` or
  /// `shebangs`), which `dprint/fileInfo` says.
  pub formats_other_files: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<NotFormattedReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approval: Option<PendingApproval>,
}

/// A plugin of a config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
  pub name: String,
  pub version: String,
  pub config_key: String,
  /// The file extensions it formats, which are without the leading period.
  pub file_extensions: Vec<String>,
  /// The file names it formats regardless of their extension.
  pub file_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileInfo {
  pub uri: Uri,
  /// Whether it's the global config file.
  pub is_global: bool,
}

/// The result of `dprint/configSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaResult {
  /// The JSON schema of the config file with the schemas of its plugins.
  pub schema: serde_json::Value,
}

/// The params of the `dprint/configChanged` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChangedParams {
  pub uri: Uri,
}
