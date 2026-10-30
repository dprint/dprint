//! The params and results of the `dprint/*` methods of the language server,
//! which an editor extension uses instead of the text document methods (see
//! docs/editor-extension-development.md).

use deno_tower_lsp::lsp_types::NumberOrString;
use deno_tower_lsp::lsp_types::Range;
use deno_tower_lsp::lsp_types::TextEdit;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::configuration::ConfigKeyMap;
use serde::Deserialize;
use serde::Serialize;

/// The name of the experimental capability that a client provides to use the
/// `dprint/*` methods and that the server provides when it supports them.
pub const EXPERIMENTAL_CAPABILITY_NAME: &str = "dprint";
/// The versions of the `dprint/*` methods the server supports, where the
/// server uses the highest one that the client also supports. A version is
/// added for a change that a client needs to know about (ex. one that would
/// break it), and fields and values may be added within a version.
pub const SUPPORTED_METHOD_VERSIONS: [u32; 1] = [1];

pub const FORMAT_METHOD: &str = "dprint/format";
pub const FILE_INFO_METHOD: &str = "dprint/fileInfo";
pub const CONFIG_INFO_METHOD: &str = "dprint/configInfo";
pub const CONFIG_SCHEMA_METHOD: &str = "dprint/configSchema";
pub const APPROVE_METHOD: &str = "dprint/approve";
pub const RESET_METHOD: &str = "dprint/reset";
pub const PENDING_APPROVALS_METHOD: &str = "dprint/pendingApprovals";
pub const FORGET_DECISIONS_METHOD: &str = "dprint/forgetDecisions";
pub const CONFIG_CHANGED_NOTIFICATION: &str = "dprint/configChanged";

/// The feature to format a document as another file (see
/// `DocumentParams::file_uri`).
pub const FILE_URI_FEATURE: &str = "fileUri";
/// The feature to have the server register the files it needs to be told
/// about with `workspace/didChangeWatchedFiles`.
pub const WATCH_FILES_FEATURE: &str = "watchFiles";
/// The features the server supports.
pub const SUPPORTED_FEATURES: [&str; 2] = [FILE_URI_FEATURE, WATCH_FILES_FEATURE];

/// The capability that an editor extension provides in the experimental
/// capabilities, which has the versions of the `dprint/*` methods it supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientExperimentalCapability {
  pub versions: Vec<u32>,
  /// Optional behavior that's opted into, where a feature that isn't known is
  /// ignored (see `SUPPORTED_FEATURES`).
  #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
  pub features: serde_json::Map<String, serde_json::Value>,
}

impl ClientExperimentalCapability {
  /// Reads the capability, which ignores the versions and features that can't
  /// be read and is `None` when it isn't an object with versions.
  pub fn from_value(value: &serde_json::Value) -> Option<Self> {
    let capability = value.as_object()?;
    let versions = capability
      .get("versions")?
      .as_array()?
      .iter()
      .filter_map(|version| version.as_u64().and_then(|version| u32::try_from(version).ok()))
      .collect::<Vec<_>>();
    let features = capability
      .get("features")
      .and_then(|features| features.as_object())
      .cloned()
      .unwrap_or_default();
    Some(Self { versions, features })
  }

  /// Gets the version of the `dprint/*` methods to use, which is the highest
  /// one that both support and `None` when there isn't one.
  pub fn negotiate_version(&self) -> Option<u32> {
    self
      .versions
      .iter()
      .copied()
      .filter(|version| SUPPORTED_METHOD_VERSIONS.contains(version))
      .max()
  }

  /// Gets the features that both the client and the server support, which
  /// are the ones the server uses.
  pub fn negotiate_features(&self) -> serde_json::Map<String, serde_json::Value> {
    self
      .features
      .iter()
      // a feature is `true` or an object of options
      .filter(|(name, value)| SUPPORTED_FEATURES.contains(&name.as_str()) && (value.as_bool() == Some(true) || value.is_object()))
      .map(|(name, _)| (name.clone(), serde_json::Value::Bool(true)))
      .collect()
  }
}

/// The capability that the server provides in the experimental capabilities
/// when it uses the `dprint/*` methods with the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerExperimentalCapability {
  /// The version of the `dprint/*` methods it uses, which is `None` when it
  /// doesn't support any of the client's versions and handles it like a
  /// language client.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub version: Option<u32>,
  /// The versions of the `dprint/*` methods the server supports (ex. to tell
  /// the user whether dprint or the editor extension needs to be updated).
  pub versions: Vec<u32>,
  /// The optional behavior it uses of what the client opted into.
  #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
  pub features: serde_json::Map<String, serde_json::Value>,
  /// Where dprint's own files are (ex. for a command that opens them).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub files: Option<DprintFiles>,
  /// The globs of the config files to watch in the workspace folders.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub config_file_globs: Vec<String>,
}

/// Where dprint's own files are, which are `None` when the directory they're
/// in can't be found.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DprintFiles {
  /// The settings file (ex. with `editor.trustedDomains`).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settings: Option<Uri>,
  /// Where the "Always" approval decisions are stored, which asks again for
  /// them once it's deleted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approvals: Option<Uri>,
  /// The global config file, which is where it would be created when it
  /// doesn't exist.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub global_config: Option<Uri>,
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
  /// The file to format the document as instead of its uri (ex. the file of a
  /// document with another scheme), which is only used with the `fileUri`
  /// feature.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub file_uri: Option<Uri>,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory, which is what the setting says when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub global_config: Option<GlobalConfigUse>,
  /// The token to report the progress of the request with (ex. downloading
  /// the plugins of the config file).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
}

/// When the global config file is used for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlobalConfigUse {
  /// When there's no config file in an ancestor directory.
  WhenNoConfigFile,
  Never,
}

impl GlobalConfigUse {
  /// Gets whether the global config file is used when there's no config file
  /// in an ancestor directory, which is the setting when it's not provided.
  pub fn resolve(value: Option<Self>, setting: bool) -> bool {
    match value {
      Some(GlobalConfigUse::WhenNoConfigFile) => true,
      Some(GlobalConfigUse::Never) => false,
      None => setting,
    }
  }
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
  /// The editor's id of the language (ex. "typescript"), which dprint knows
  /// for VS Code's language ids.
  pub id: String,
  /// The file extensions the editor associates with the language, which may
  /// have the leading period (ex. ".ts" or "ts").
  #[serde(default, deserialize_with = "deserialize_null_as_default", skip_serializing_if = "Vec::is_empty")]
  pub file_extensions: Vec<String>,
  /// The file names the editor associates with the language (ex. "Dockerfile").
  #[serde(default, deserialize_with = "deserialize_null_as_default", skip_serializing_if = "Vec::is_empty")]
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
  /// The config to use instead of what's in the config file, which is only
  /// the editor's settings for the document (see `OVERRIDE_CONFIG_KEYS`).
  #[serde(default, deserialize_with = "deserialize_null_as_default", skip_serializing_if = "ConfigKeyMap::is_empty")]
  pub override_config: ConfigKeyMap,
}

/// The keys `overrideConfig` may have, which are the editor's settings for
/// how a document is indented. Other config could have a plugin do something
/// else (ex. run a different command), which the user didn't approve.
pub const OVERRIDE_CONFIG_KEYS: [&str; 2] = ["indentWidth", "useTabs"];

/// The result of `dprint/format`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatResponse {
  /// The config file that was used for the document, which is `None` when
  /// there isn't one.
  pub config_file: Option<ConfigFileInfo>,
  #[serde(flatten)]
  pub result: FormatResult,
}

/// What formatting a document did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum FormatResult {
  /// The edits of the text that was sent, which don't overlap and are in
  /// order. It's empty when the text is already formatted.
  #[serde(rename_all = "camelCase")]
  Formatted { edits: Vec<TextEdit> },
  /// Why the document wasn't formatted.
  NotFormatted(NotFormatted),
  /// Why formatting the document failed.
  #[serde(rename_all = "camelCase")]
  Failed {
    reason: FormatFailedReason,
    message: String,
    /// When to show the message (see `FormatFailedReason::show`).
    show: ShowPolicy,
    severity: Severity,
    /// The plugin that failed when it's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin: Option<FilePlugin>,
  },
}

/// Why a document isn't formatted, which is the same for each method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFormatted {
  pub reason: NotFormattedReason,
  /// The message to show the user about why it wasn't formatted (ex. for a
  /// command the user ran).
  pub message: String,
  /// When to show the message.
  pub show: ShowPolicy,
  /// How to show the message.
  pub severity: Severity,
  /// Whether the editor extension keeps acting as the formatter of the
  /// document so that the user is told why it isn't formatted (ex. the config
  /// file has an error), which is otherwise left to the editor's other
  /// formatters (ex. the config file doesn't format it).
  pub claim: bool,
  /// What the user needs to approve to format it.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub approvals: Vec<PendingApproval>,
}

impl NotFormatted {
  pub fn new(reason: NotFormattedReason, message: Option<String>, approvals: Vec<PendingApproval>) -> Self {
    Self {
      show: reason.show(),
      severity: reason.severity(),
      message: message.unwrap_or_else(|| "dprint did not format the file.".to_string()),
      claim: match reason {
        // the user isn't asked about what they denied, so the document is
        // only formatted once they ask for it to be
        NotFormattedReason::ApprovalRequired => approvals.iter().any(|approval| !approval.denied),
        reason => reason.is_claimed(),
      },
      reason,
      approvals,
    }
  }

  /// Leaves the document to the editor's other formatters (ex. it's not known
  /// whether the config file formats it).
  pub fn unclaimed(mut self) -> Self {
    self.claim = false;
    self
  }
}

/// How to show a message to the user (ex. an error or information).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
  Error,
  Warning,
  Info,
}

/// When to show a message to the user, which is only for a request the user
/// made (ex. a format on save or a command) and not one the editor made
/// without the user (ex. `dprint/fileInfo` when a document is opened).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShowPolicy {
  /// Each time (ex. a plugin failed).
  Always,
  /// Only for a command the user ran (ex. a syntax error, which would
  /// otherwise be shown on each format on save).
  Command,
  /// Once for the config file until the message changes or the server sends
  /// `dprint/configChanged` for it (ex. an error in the config file).
  OncePerConfigFile,
}

/// Whether a document or the documents of a config file are formatted.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormatStatus {
  #[default]
  Ready,
  NotFormatted(NotFormatted),
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

impl FormatFailedReason {
  /// Gets when to show the message of the failure.
  pub fn show(self) -> ShowPolicy {
    match self {
      FormatFailedReason::FormatError => ShowPolicy::Command,
      FormatFailedReason::PluginError => ShowPolicy::Always,
    }
  }

  /// Gets how to show the message of the failure.
  pub fn severity(self) -> Severity {
    match self {
      // usually a syntax error, which the editor shows too
      FormatFailedReason::FormatError => Severity::Warning,
      FormatFailedReason::PluginError => Severity::Error,
    }
  }
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
  /// approvals say.
  ApprovalRequired,
  /// The document can't be formatted (ex. its uri isn't a file, an untitled
  /// document, or a notebook cell).
  UnsupportedDocument,
  /// The document isn't a file, so its language is needed to format it.
  LanguageRequired,
  /// The config file that was asked about doesn't exist (ex. it was deleted).
  ConfigFileNotFound,
}

impl NotFormattedReason {
  /// Gets when to show the message of the reason.
  pub fn show(self) -> ShowPolicy {
    match self {
      NotFormattedReason::ConfigError | NotFormattedReason::ConfigDiagnostics => ShowPolicy::OncePerConfigFile,
      NotFormattedReason::NoConfigFile
      | NotFormattedReason::GlobalConfigNotAllowed
      | NotFormattedReason::NotMatched
      | NotFormattedReason::NoPlugin
      | NotFormattedReason::RangeFormattingNotSupported
      | NotFormattedReason::ApprovalRequired
      | NotFormattedReason::UnsupportedDocument
      | NotFormattedReason::LanguageRequired
      | NotFormattedReason::ConfigFileNotFound => ShowPolicy::Command,
    }
  }

  /// Gets how to show the message of the reason.
  pub fn severity(self) -> Severity {
    match self {
      NotFormattedReason::ConfigError | NotFormattedReason::ConfigDiagnostics => Severity::Error,
      NotFormattedReason::ApprovalRequired | NotFormattedReason::ConfigFileNotFound => Severity::Warning,
      NotFormattedReason::NoConfigFile
      | NotFormattedReason::GlobalConfigNotAllowed
      | NotFormattedReason::NotMatched
      | NotFormattedReason::NoPlugin
      | NotFormattedReason::RangeFormattingNotSupported
      | NotFormattedReason::UnsupportedDocument
      | NotFormattedReason::LanguageRequired => Severity::Info,
    }
  }

  /// Gets whether the editor extension keeps acting as the formatter of a
  /// document that isn't formatted for the reason, which depends on the
  /// approvals for `ApprovalRequired`.
  fn is_claimed(self) -> bool {
    match self {
      // the config file formats it, but it can't be right now
      NotFormattedReason::RangeFormattingNotSupported | NotFormattedReason::ConfigDiagnostics | NotFormattedReason::ConfigError => true,
      NotFormattedReason::ApprovalRequired => true,
      NotFormattedReason::NoConfigFile
      | NotFormattedReason::GlobalConfigNotAllowed
      | NotFormattedReason::NotMatched
      | NotFormattedReason::NoPlugin
      | NotFormattedReason::UnsupportedDocument
      | NotFormattedReason::LanguageRequired
      | NotFormattedReason::ConfigFileNotFound => false,
    }
  }
}

/// Something the user needs to approve before a config file is used, which a
/// client approves with `dprint/approve` after asking the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
  /// Identifies exactly what's approved, which changes when it does (ex. the
  /// configuration of a process plugin).
  pub id: String,
  pub kind: ApprovalKind,
  /// The config file that needs it, which is what the user approves for.
  pub config_file: ConfigFileInfo,
  /// What's downloaded for `download` or run for `processPlugins`.
  pub items: Vec<ApprovalItem>,
  /// The decisions the user can make in the order to show them.
  pub decisions: Vec<ApprovalDecisionOption>,
  /// Whether the user denied it, which means they shouldn't be asked unless
  /// they ask for it (ex. a command).
  pub denied: bool,
  /// What the user decided before, which is `deny` or `alwaysDeny` when it's
  /// denied.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub previous_decision: Option<ApprovalDecision>,
  /// The message to show the user when asking.
  pub message: String,
}

/// Something that's downloaded or run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalItem {
  /// The url, path, or npm specifier (ex. of a plugin with its checksum).
  pub specifier: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kind: Option<ApprovalItemKind>,
  /// The name of a plugin that was set up before.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  /// The version of a plugin that was set up before.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub version: Option<String>,
  /// The config key of a plugin that was set up before.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_key: Option<String>,
  /// The configuration a process plugin that was set up before runs with
  /// (ex. the commands of dprint-plugin-exec).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub configuration: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalItemKind {
  Plugin,
  /// A config file that a config file extends.
  Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
  /// Downloading from domains that aren't trusted.
  Downloads,
  /// Running process plugins, which aren't sandboxed.
  ProcessPlugins,
}

/// A decision the user can make about an approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecisionOption {
  pub id: ApprovalDecision,
  /// What to show the user, which is for a decision the client doesn't know.
  pub label: String,
  /// Whether it allows what's approved, which is otherwise denied.
  pub allows: bool,
  /// Whether it's remembered after the server stops (in the
  /// `editor-approvals.json` file), which is otherwise only for the session.
  pub always: bool,
  /// What it does, which is to show with it (ex. as a tooltip).
  pub description: String,
}

/// The params of `dprint/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveParams {
  pub id: String,
  pub decision: ApprovalDecision,
  /// The token to report the progress of setting up what was allowed with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
}

/// What the user decided, which is only remembered for the session unless it's
/// `alwaysAllow` or `alwaysDeny`.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveResult {
  /// What needs to be approved now for the config file, which is empty when
  /// nothing does. It has what was denied when the decision was to deny it,
  /// and otherwise what else needs to be approved (ex. a config file that a
  /// downloaded one extends or a process plugin the user denied before).
  pub approvals: Vec<PendingApproval>,
  /// Whether the config file is used now, which isn't `ready` when it has an
  /// error or was deleted even when nothing needs to be approved.
  pub status: FormatStatus,
}

/// The error code of `dprint/approve` for an approval that isn't pending
/// anymore because what needs to be approved changed, which is the language
/// server protocol's `RequestFailed`.
pub const APPROVAL_CHANGED_ERROR_CODE: i64 = -32803;

/// The data of an error, which is tagged by its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ErrorData {
  /// An approval isn't pending anymore.
  #[serde(rename_all = "camelCase")]
  ApprovalChanged {
    /// What needs to be approved for the config file now when it's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approvals: Option<Vec<PendingApproval>>,
    /// Whether the config file is used now when it's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<FormatStatus>,
  },
}

/// The result of `dprint/reset`, which doesn't have anything yet.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {}

/// The params of `dprint/pendingApprovals`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalsParams {
  /// The config file to get what needs to be approved for, which is every
  /// config file that was used when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
}

/// The result of `dprint/pendingApprovals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalsResult {
  /// What needs to be approved for the config files, which includes what the
  /// user denied.
  pub approvals: Vec<PendingApproval>,
  /// Whether the config file is used, which is only for one config file.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<FormatStatus>,
}

/// The params of `dprint/forgetDecisions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetDecisionsParams {
  /// The config file to forget the decisions of, which is every config file
  /// when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
}

/// The result of `dprint/forgetDecisions`, which doesn't have anything yet.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetDecisionsResult {}

/// The params of `dprint/fileInfo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoParams {
  #[serde(flatten)]
  pub document: DocumentParams,
  /// The text of the document, which is otherwise read from the file when it's
  /// needed (ex. the shebang of an extensionless file).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub text: Option<String>,
}

/// The result of `dprint/fileInfo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoResult {
  /// The config file that's used for the file, which is `None` when there isn't one.
  pub config_file: Option<ConfigFileInfo>,
  /// The plugins that format the file in the order they do, which is empty
  /// when it isn't formatted.
  pub plugins: Vec<FilePlugin>,
  pub status: FormatStatus,
}

/// A plugin that formats a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePlugin {
  pub name: String,
  pub version: String,
  pub config_key: String,
}

/// The params of `dprint/configInfo` and `dprint/configSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigParams {
  /// The config file, which is provided instead of a document.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
  /// A document the config file is used for, which is provided instead of
  /// the config file.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub text_document: Option<DocumentIdentifier>,
  /// The language of the document (see `DocumentParams`).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub language: Option<DocumentLanguage>,
  /// The notebook when the document is one of its cells.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notebook_document: Option<DocumentIdentifier>,
  /// The file to format the document as (see `DocumentParams`).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub file_uri: Option<Uri>,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory, which is what the setting says when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub global_config: Option<GlobalConfigUse>,
  /// The token to report the progress of the request with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
}

/// What identifies a config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileIdentifier {
  pub uri: Uri,
  /// The directory the global config file is used for (see
  /// `ConfigFileInfo::dir_uri`), which is the root of the current directory
  /// when `None`. It's where the config file is found from for the one the
  /// server was started with, and it's ignored for a local config file.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub dir_uri: Option<Uri>,
}

/// The result of `dprint/configInfo`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfoResult {
  pub config_file: Option<ConfigFileInfo>,
  /// The plugins of the config file, which is `None` when they aren't known
  /// (ex. they need to be approved and weren't set up before).
  pub plugins: Option<Vec<PluginInfo>>,
  /// The files the plugins may format, which are empty when the plugins
  /// aren't known.
  pub file_patterns: Vec<FilePattern>,
  /// The other files the config file formats, which `filePatterns` can't
  /// match and only `dprint/fileInfo` says.
  pub other_files: Vec<OtherFiles>,
  /// The local files besides the config file that it was resolved from (ex.
  /// a config file it extends), which the editor watches for changes too.
  pub watch_uris: Vec<Uri>,
  pub status: FormatStatus,
}

/// Files that plugins may format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePattern {
  /// A glob relative to the directory of the config file (`dirUri`), which
  /// matches file extensions and names case insensitively.
  pub glob: String,
  /// The config keys of the plugins that format the files.
  pub config_keys: Vec<String>,
}

/// Files that a config file formats which a glob can't match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherFiles {
  pub kind: OtherFilesKind,
  /// The shebangs or the associations that can't be made relative to the
  /// directory of the config file.
  pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OtherFilesKind {
  /// Extensionless files with these shebangs.
  Shebangs,
  /// The files of these associations of the plugins.
  Associations,
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
  /// Whether it formats a file in addition to the plugin that formats it
  /// instead of formatting it on its own (ex. sorting the keys of a
  /// `package.json`).
  pub additive: bool,
  /// How it formats a range.
  pub range_formatting: RangeFormatting,
  pub help_url: String,
  /// Where its newer version is said to be.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub update_url: Option<String>,
}

/// How a plugin formats a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RangeFormatting {
  /// It decides what to format around the range.
  Supported,
  /// It formats the whole lines of the range.
  WholeLines,
  /// It doesn't format a range.
  Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileInfo {
  pub uri: Uri,
  /// The directory whose files it formats, which is the directory it's in
  /// unless that's a `.config` directory and the directory it's used for when
  /// it's the global config file (ex. the root of a drive). It's the directory
  /// its patterns are relative to for a remote or npm config file.
  pub dir_uri: Uri,
  pub kind: ConfigFileKind,
  /// Whether it's in a workspace folder, which a config file in an ancestor
  /// directory of the workspace folders or the global config file isn't.
  pub in_workspace: bool,
}

/// How a config file was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigFileKind {
  /// It's in an ancestor directory of the file.
  Local,
  /// It's the global config file.
  Global,
  /// It's the one the server was started with (`dprint lsp --config <path>`).
  Cli,
}

/// The result of `dprint/configSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaResult {
  /// The JSON schema of the config file with the schemas of its plugins.
  pub schema: serde_json::Value,
  /// Whether it has the schema of each plugin. It doesn't when the config file
  /// can't be used (ex. its plugins need to be approved) or a plugin's schema
  /// didn't download in time, which sends `dprint/configChanged` once it did.
  pub complete: bool,
}

/// The params of `dprint/reset`, which don't have anything yet.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetParams {}

/// The params of the `dprint/configChanged` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChangedParams {
  /// The config file, which is `None` when every config file may have changed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub uri: Option<Uri>,
  /// The directory the config file is used for (see `ConfigFileInfo::dir_uri`),
  /// which is every directory it's used for when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub dir_uri: Option<Uri>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<ConfigChangeReason>,
  /// What changed, which is `None` when anything may have.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub changes: Option<Vec<ConfigChange>>,
}

/// What changed about a config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigChange {
  /// Only its schema (ex. a plugin's schema downloaded).
  Schema,
}

/// How a config file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigChangeReason {
  Created,
  /// What it resolves to changed (ex. it was edited, a config file it extends
  /// changed, or something it needed was approved).
  Changed,
  Deleted,
}

/// Deserializes `null` as the default value (ex. an empty list), which is
/// what's used when the field isn't provided.
fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
  D: serde::Deserializer<'de>,
  T: Default + Deserialize<'de>,
{
  Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use super::*;

  #[test]
  fn should_serialize_a_format_result_that_needs_approval() {
    let result = FormatResult::NotFormatted(NotFormatted::new(
      NotFormattedReason::ApprovalRequired,
      Some("message".to_string()),
      vec![PendingApproval {
        id: "1".to_string(),
        kind: ApprovalKind::ProcessPlugins,
        config_file: ConfigFileInfo {
          uri: "file:///dprint.json".parse().unwrap(),
          dir_uri: "file:///".parse().unwrap(),
          kind: ConfigFileKind::Local,
          in_workspace: true,
        },
        items: vec![ApprovalItem {
          specifier: "https://plugins.dprint.dev/exec.json@checksum".to_string(),
          kind: Some(ApprovalItemKind::Plugin),
          name: Some("dprint-plugin-exec".to_string()),
          version: Some("0.5.0".to_string()),
          config_key: Some("exec".to_string()),
          configuration: Some(json!({ "commands": [] })),
        }],
        decisions: vec![ApprovalDecisionOption {
          id: ApprovalDecision::Allow,
          label: "Allow".to_string(),
          allows: true,
          always: false,
          description: "description".to_string(),
        }],
        denied: false,
        previous_decision: None,
        message: "message".to_string(),
      }],
    ));
    let response = FormatResponse { config_file: None, result };
    assert_eq!(
      serde_json::to_value(&response).unwrap(),
      json!({
        "configFile": null,
        "kind": "notFormatted",
        "reason": "approvalRequired",
        "message": "message",
        "show": "command",
        "severity": "warning",
        "claim": true,
        "approvals": [{
          "id": "1",
          "kind": "processPlugins",
          "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": true },
          "items": [{
            "specifier": "https://plugins.dprint.dev/exec.json@checksum",
            "kind": "plugin",
            "name": "dprint-plugin-exec",
            "version": "0.5.0",
            "configKey": "exec",
            "configuration": { "commands": [] },
          }],
          "decisions": [{ "id": "allow", "label": "Allow", "allows": true, "always": false, "description": "description" }],
          "denied": false,
          "message": "message",
        }],
      })
    );
  }

  #[test]
  fn should_serialize_the_status_of_a_config_file() {
    let result = ConfigInfoResult {
      config_file: Some(ConfigFileInfo {
        uri: "file:///a/.config/dprint.json".parse().unwrap(),
        dir_uri: "file:///a".parse().unwrap(),
        kind: ConfigFileKind::Local,
        in_workspace: false,
      }),
      plugins: None,
      file_patterns: Vec::new(),
      other_files: Vec::new(),
      watch_uris: Vec::new(),
      status: FormatStatus::NotFormatted(NotFormatted::new(NotFormattedReason::ConfigError, Some("message".to_string()), Vec::new())),
    };
    assert_eq!(
      serde_json::to_value(&result).unwrap(),
      json!({
        "configFile": { "uri": "file:///a/.config/dprint.json", "dirUri": "file:///a", "kind": "local", "inWorkspace": false },
        "plugins": null,
        "filePatterns": [],
        "otherFiles": [],
        "watchUris": [],
        "status": {
          "kind": "notFormatted",
          "reason": "configError",
          "message": "message",
          "show": "oncePerConfigFile",
          "severity": "error",
          "claim": true,
        },
      })
    );
    assert_eq!(serde_json::to_value(FormatStatus::Ready).unwrap(), json!({ "kind": "ready" }));
    assert_eq!(
      serde_json::to_value(ApproveResult {
        approvals: Vec::new(),
        status: FormatStatus::Ready,
      })
      .unwrap(),
      json!({ "approvals": [], "status": { "kind": "ready" } })
    );
  }

  #[test]
  fn should_deserialize_the_params_of_the_methods() {
    assert_eq!(
      serde_json::from_value::<ApproveParams>(json!({ "id": "1", "decision": "alwaysDeny" })).unwrap(),
      ApproveParams {
        id: "1".to_string(),
        decision: ApprovalDecision::AlwaysDeny,
        work_done_token: None,
      }
    );
    let params = serde_json::from_value::<FormatParams>(json!({
      "textDocument": { "uri": "untitled:Untitled-1" },
      "language": { "id": "typescript" },
      "globalConfig": "whenNoConfigFile",
      "text": "text",
      "ranges": [{ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }],
      "overrideConfig": { "indentWidth": 4 },
    }))
    .unwrap();
    assert_eq!(params.document.text_document.uri.as_str(), "untitled:Untitled-1");
    assert_eq!(params.document.global_config, Some(GlobalConfigUse::WhenNoConfigFile));
    assert_eq!(params.ranges.unwrap().len(), 1);
    assert_eq!(params.override_config.len(), 1);
    let params = serde_json::from_value::<ConfigParams>(json!({ "configFile": { "uri": "file:///dprint.json" } })).unwrap();
    assert_eq!(params.config_file.unwrap().uri.as_str(), "file:///dprint.json");
    assert_eq!(params.global_config, None);
    let params = serde_json::from_value::<ConfigParams>(json!({ "textDocument": { "uri": "file:///file.ts" }, "globalConfig": "never" })).unwrap();
    assert_eq!(params.text_document.unwrap().uri.as_str(), "file:///file.ts");
    assert_eq!(params.global_config, Some(GlobalConfigUse::Never));
    // null is the same as not providing it
    let params = serde_json::from_value::<FormatParams>(json!({
      "textDocument": { "uri": "untitled:Untitled-1" },
      "language": { "id": "typescript", "fileExtensions": null, "fileNames": null },
      "text": "text",
      "overrideConfig": null,
    }))
    .unwrap();
    assert!(params.override_config.is_empty());
    assert!(params.document.language.unwrap().file_extensions.is_empty());
    assert_eq!(serde_json::from_value::<Option<ResetParams>>(json!({})).unwrap(), Some(ResetParams {}));
  }

  #[test]
  fn should_negotiate_the_features_both_support() {
    let capability = ClientExperimentalCapability::from_value(&json!({
      "versions": [1],
      "features": { "fileUri": true, "watchFiles": { "option": 1 }, "unknown": true, "other": false },
    }))
    .unwrap();
    assert_eq!(
      serde_json::Value::Object(capability.negotiate_features()),
      json!({ "fileUri": true, "watchFiles": true })
    );
  }

  #[test]
  fn should_resolve_when_the_global_config_file_is_used() {
    assert!(GlobalConfigUse::resolve(None, true));
    assert!(!GlobalConfigUse::resolve(None, false));
    assert!(GlobalConfigUse::resolve(Some(GlobalConfigUse::WhenNoConfigFile), false));
    assert!(!GlobalConfigUse::resolve(Some(GlobalConfigUse::Never), true));
  }
}
