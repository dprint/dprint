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
pub const DECISIONS_METHOD: &str = "dprint/decisions";
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
  /// The id of the server's session, which is unique to the server process
  /// (ex. a random one for each `dprint` executable of each editor window) and
  /// keeps what the user decides for the session when the same server is
  /// started again with it once its process exited (ex. it crashed or was
  /// killed), and not for a restart, which is a new session. It's only used
  /// when it's a valid string (see `is_valid_session_id`).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub session_id: Option<serde_json::Value>,
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
    let session_id = capability.get("sessionId").filter(|session_id| !session_id.is_null()).cloned();
    Some(Self {
      versions,
      features,
      session_id,
    })
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
  /// are the ones the server uses. Each is `true` because this version
  /// doesn't use any of their options, which is what `true` means to a
  /// client that sent an object of options.
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
  /// doesn't support any of the client's versions, which isn't handled like a
  /// language client (every method fails with `MethodNotFound`).
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
  /// The globs of the config files to watch in the workspace folders, which
  /// are empty for `dprint lsp --config` and `None` without a `version`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file_globs: Option<Vec<String>>,
  /// The names of the config files that are looked up in a directory (ex.
  /// one of the ancestor directories of the workspace folders), which may
  /// have names added later, are empty for `dprint lsp --config`, and are
  /// `None` without a `version`. A case insensitive file system (Windows and
  /// macOS by default) finds them in any case, which the server does too.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file_names: Option<Vec<String>>,
  /// The names of the config files that are looked up in the `.config`
  /// directory of a directory, like `config_file_names`. The home directory's
  /// `.config` is an ordinary directory, which isn't looked in for them.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub dot_config_file_names: Option<Vec<String>>,
  /// What to tell the user when there isn't a `version` (ex. that dprint or
  /// the editor extension needs to be updated).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// Where dprint's own files are, which are `None` when the directory they're
/// in can't be found.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DprintFiles {
  /// The settings file (ex. with `editor.trustedDomains`).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settings_uri: Option<Uri>,
  /// Where the "Always" approval decisions are stored, which are forgotten
  /// with `dprint/forgetDecisions`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approvals_uri: Option<Uri>,
  /// The global config file, which is where `dprint init --global` would
  /// create it (`dprint.jsonc`) when it doesn't exist. It's from when the
  /// server started.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub global_config_uri: Option<Uri>,
  /// The names of the global config file in the order they're looked for in
  /// its directory, which a newer version may add to.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub global_config_file_names: Vec<String>,
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
  pub notebook_document: Option<NotebookIdentifier>,
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

/// What identifies the notebook of a notebook cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookIdentifier {
  pub uri: Uri,
  /// The file to format the notebook's cells as cells of instead of its uri,
  /// which is only used with the `fileUri` feature.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub file_uri: Option<Uri>,
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
  Formatted {
    edits: Vec<TextEdit>,
    /// Something to tell the user about the document that was formatted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notice: Option<Notice>,
  },
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
    /// Whether the editor extension keeps acting as the formatter of the
    /// document (see `NotFormatted::claim`), which it does for a failure.
    claim: bool,
    /// What the user can do about it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<Action>,
    /// The plugin that failed when it's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin: Option<FilePlugin>,
  },
}

/// Something to tell the user about the result of a request (ex. a warning
/// about a document that was formatted, or a decision that couldn't be stored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
  pub message: String,
  pub show: ShowPolicy,
  pub severity: Severity,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub actions: Vec<Action>,
}

/// Something the user can do about a message, which is offered with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
  /// Opens a file in the editor (ex. the config file that has an error).
  #[serde(rename_all = "camelCase")]
  OpenUri { label: String, uri: Uri },
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
  /// What the user can do about it.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub actions: Vec<Action>,
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
      actions: Vec::new(),
    }
  }

  /// Offers what the user can do about it.
  pub fn with_actions(mut self, actions: Vec<Action>) -> Self {
    self.actions = actions;
    self
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormatStatus {
  Ready,
  NotFormatted(NotFormatted),
}

impl Default for FormatStatus {
  fn default() -> Self {
    Self::ready()
  }
}

impl FormatStatus {
  pub fn ready() -> Self {
    Self::Ready
  }
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
  ConfigFileMissing,
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
      | NotFormattedReason::ConfigFileMissing => ShowPolicy::Command,
    }
  }

  /// Gets how to show the message of the reason.
  pub fn severity(self) -> Severity {
    match self {
      NotFormattedReason::ConfigError | NotFormattedReason::ConfigDiagnostics => Severity::Error,
      NotFormattedReason::ApprovalRequired | NotFormattedReason::ConfigFileMissing => Severity::Warning,
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
      | NotFormattedReason::ConfigFileMissing => false,
    }
  }
}

/// Something the user needs to approve before a config file is used, which a
/// client approves with `dprint/approve` after asking the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
  /// Identifies exactly what's approved, which changes when what's shown does
  /// (ex. the configuration of a process plugin, or once its process plugins
  /// are set up) and is the same in another server of the same version of
  /// dprint, but may change between versions, so the same approval is
  /// recognized by what it approves instead (its config file, kind, and the
  /// specifiers of its items).
  pub id: String,
  pub kind: ApprovalKind,
  /// A short description of what's approved (ex. for a status bar item or
  /// the heading of a prompt).
  pub title: String,
  /// The config file that needs it, which is what the user approves for.
  pub config_file: ConfigFileInfo,
  /// What's downloaded for `downloads` or run for `processPlugins`.
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
  /// The message to show the user when asking, which names everything that's
  /// approved apart from what the process plugins run with, and quotes the text
  /// it has from the project (see `quote_text`).
  pub message: String,
  /// The trusted configuration of the config file that the process plugins
  /// that aren't set up run with once they're allowed, which is what the user
  /// approves for them because what each one runs with isn't known until it's
  /// set up. It includes what the config file extends.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trusted_configuration: Option<serde_json::Value>,
}

/// Something that's downloaded or run.
///
/// It's also kept in the session file (see `SessionFile`), where another
/// version of dprint may read it, so a change to it is a change to that
/// format too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalItem {
  /// The url, path, or npm specifier (ex. of a plugin with its checksum), or
  /// the text of the setting of an `npmrcProxy`. A path that reaches another
  /// machine through a symlink is followed by ` -> ` and the path on that
  /// machine, and an `->` in a path is escaped so the one between them is the
  /// only one (see `DownloadPolicy::ensure_allows_path`). It's from the
  /// project, so it's escaped (see `escape_text`), and each path of a
  /// `networkPath` is escaped like the text the message quotes (see
  /// `get_path_download`).
  pub specifier: String,
  /// What it is, which is `plugin` for an npm registry when what was
  /// downloaded from it isn't known (ex. a decision kept by another version of
  /// dprint).
  pub kind: ApprovalItemKind,
  /// For the download of an npm registry itself (any registry, including the
  /// default one), it's the registry, which is the specifier too, and allowing
  /// it allows every url under it unlike another url, while the item's `kind`
  /// is what's downloaded from it (`plugin` or `config`). Otherwise, it's the
  /// npm registry an npm plugin is from when it isn't the default one (ex. a
  /// project's `.npmrc` sets another one for its scope). It's escaped like the
  /// specifier.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub registry: Option<String>,
  /// The uri of the `.npmrc` file that sets an `npmrcProxy` as the editor
  /// knows it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub npmrc_uri: Option<Uri>,
  /// The uri of the path in the project of a `networkPath` that reaches the
  /// other machine through a symlink (the path before ` -> `) as the editor
  /// knows it, which is local while what it reaches isn't, so the editor
  /// doesn't open or read it before it's allowed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub local_uri: Option<Uri>,
  /// The plugin when it was set up before.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub plugin: Option<FilePlugin>,
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
  /// A proxy that an `.npmrc` file other than the user's own sets (ex. the
  /// project's), which downloads are sent through once it's allowed.
  NpmrcProxy,
  /// A file or directory on another machine that's read (ex. a plugin, a
  /// config file, an npm package, or an `.npmrc` file), which may be in the
  /// project and reach the machine through a symlink.
  NetworkPath,
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
  /// The decision, which is what `dprint/approve` is sent with.
  pub decision: ApprovalDecision,
  /// What to show the user, which is for a decision the client doesn't know.
  pub label: String,
  /// Whether it allows what's approved, which is otherwise denied.
  pub allows: bool,
  /// Whether it's remembered after the session ends (in the
  /// `editor-approvals.json` file), which is otherwise only for the session.
  pub always: bool,
  /// What it does, which is to show with it (ex. as a tooltip).
  pub description: String,
}

/// The params of `dprint/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveParams {
  /// The id of the approval.
  pub id: String,
  /// The `decision` of one of the approval's decisions.
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

/// What needs to be approved for a config file now and whether it's used,
/// which is what `dprint/approve`, `dprint/pendingApprovals` for a config
/// file, and an approval that changed (`-32803`) provide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApprovals {
  /// The config file, which is `None` when it can't be resolved anymore (ex.
  /// it was deleted).
  pub config_file: Option<ConfigFileInfo>,
  /// What needs to be approved now for the config file, which is empty when
  /// nothing does. After a decision, it has what was denied when the decision
  /// was to deny it and the denial was kept (a denial of downloads that were
  /// each allowed exactly since isn't, see `ApproveResult::notice`), and
  /// otherwise what else needs to be approved (ex. a config file that a
  /// downloaded one extends or a process plugin the user denied before).
  pub approvals: Vec<PendingApproval>,
  /// Whether the config file is used now, which isn't `ready` when it has an
  /// error or was deleted even when nothing needs to be approved.
  pub status: FormatStatus,
}

/// The result of `dprint/approve`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveResult {
  /// The config file the approval was for and what needs to be approved for
  /// it now.
  #[serde(flatten)]
  pub config_approvals: ConfigApprovals,
  /// What to tell the user about the decision or one made before for the
  /// same config file (ex. a denial wasn't kept because what it denied was
  /// each allowed exactly for another config file since, so it's still
  /// allowed for the session, or a decision couldn't be stored, so it's only
  /// used for the session, or the session file couldn't be written or
  /// deleted, or the session's decisions aren't kept yet because another
  /// server that's running uses the session id or its lock can't be taken).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notice: Option<Notice>,
}

/// The error code of `dprint/approve` for an approval that isn't pending
/// anymore because what needs to be approved changed, so the decision wasn't
/// used, which is the language server protocol's `RequestFailed`.
pub const APPROVAL_CHANGED_ERROR_CODE: i64 = -32803;

/// The data of an error, which is tagged by its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ErrorData {
  /// An approval isn't pending anymore (see `APPROVAL_CHANGED_ERROR_CODE`).
  #[serde(rename_all = "camelCase")]
  ApprovalChanged {
    /// The config file the approval was for and what needs to be approved for
    /// it now, which is `None` when that isn't known (ex. the id isn't of an
    /// approval the server provided).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current: Option<ConfigApprovals>,
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
  /// The token to report the progress of the request with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
}

/// The result of `dprint/pendingApprovals`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalsResult {
  /// What needs to be approved for the config files, which includes what the
  /// user denied.
  pub approvals: Vec<PendingApproval>,
  /// The config file, which is only for one config file and is `Some(None)`
  /// when it can't be resolved (see `ConfigApprovals`).
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_some")]
  pub config_file: Option<Option<ConfigFileInfo>>,
  /// Whether the config file is used, which is only for one config file.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<FormatStatus>,
}

/// The params of `dprint/forgetDecisions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetDecisionsParams {
  /// The config file to forget the decisions of, which is every config file
  /// when `None`. Without a `dir_uri`, it's the config file for each directory
  /// it's used for (ex. the global config file on each drive).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
  /// The token to report the progress of the request with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
}

/// The result of `dprint/forgetDecisions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetDecisionsResult {
  /// The decisions that were forgotten.
  pub decisions: Vec<Decision>,
  /// What to tell the user about forgetting them (ex. the stored decisions
  /// couldn't be forgotten, or the session file couldn't be written or
  /// deleted, or the session's decisions aren't kept yet because another
  /// server that's running uses the session id or its lock can't be taken).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notice: Option<Notice>,
}

/// The params of `dprint/decisions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionsParams {
  /// The config file to get the decisions of, which is every config file
  /// when `None`. Without a `dir_uri`, it's the config file for each directory
  /// it's used for (ex. the global config file on each drive).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
}

/// The result of `dprint/decisions`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionsResult {
  pub decisions: Vec<Decision>,
  /// Whether the settings file allows every process plugin without asking
  /// (`editor.allowProcessPlugins`).
  pub allow_process_plugins: bool,
}

/// Something the user decided about what a config file needs approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
  pub kind: ApprovalKind,
  /// The config file it's for, which is `None` for a download that's allowed
  /// for every config file, and for a process plugin decision whose config
  /// file isn't a path or url this version can make a uri of (ex. one stored
  /// on another operating system). A decision without one can't be forgotten
  /// on its own. It doesn't have a `dir_uri` when it's for each directory the
  /// config file is used for (ex. the global config file).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
  /// The urls, paths, or specifiers it's for like the `specifier`s of an
  /// approval's items (ex. the process plugins or the setting of a proxy).
  pub specifiers: Vec<String>,
  /// What it's for like an approval's items in the order of the specifiers,
  /// which is usually provided for downloads so a proxy or a path on another
  /// machine can be told apart from a url and shown with its file (an
  /// `npmrcProxy` with its `npmrc_uri`, or a `networkPath` with its
  /// `local_uri`). It's missing when they aren't known (ex. a denial kept by
  /// another version of dprint).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub items: Option<Vec<ApprovalItem>>,
  pub allows: bool,
  /// Whether it's remembered after the session ends, which includes running
  /// dprint with the config file (ex. `dprint fmt`).
  pub always: bool,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
  pub notebook_document: Option<NotebookIdentifier>,
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
///
/// It's also kept in the session file (see `SessionFile`), where another
/// version of dprint may read it, so a change to it is a change to that
/// format too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileIdentifier {
  pub uri: Uri,
  /// The directory the config file is used for (see `ConfigFileInfo::dir_uri`),
  /// which together with the uri identifies it. It's the directory the
  /// global config file is used for, which is the root of the current
  /// directory when `None`, and the directory to find the config file the
  /// server was started with from (`kind: "cli"`), which is the current
  /// directory when `None`. For `dprint/decisions` and
  /// `dprint/forgetDecisions`, a config file without one is of each directory
  /// it's used for instead.
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
  /// The local files that the config file was resolved from (ex. a config
  /// file it extends), and the canonical path of a local config file when the
  /// editor's watcher of the config files doesn't report a change there, which
  /// is when it's a symlink (or is in a `.config` directory that's one), when
  /// its names are in another case on disk, and when it's reached through a
  /// directory that's a symlink the editor doesn't watch through. The editor
  /// watches them too by their directories. Each is the editor's path
  /// when that's through a workspace folder or one of its ancestors that it
  /// knows by another path, and the canonical path otherwise, because the
  /// editor doesn't watch through another symlink. A file whose directory may
  /// not be read is left out.
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
  /// The config file, which is the path a local one was found or asked about
  /// at when it's a symlink, not what it points to (see
  /// `get_config_file_found_path`). It's always a local one, because the
  /// server only finds local config files and only starts with one (see
  /// `get_config_override_path`).
  pub uri: Uri,
  /// The directory whose files it formats, which is the directory it's in
  /// unless that's a `.config` directory (other than the home directory's),
  /// even when it's a symlink to a file in another directory, and the
  /// directory it's used for when it's the global config file (ex. the root
  /// of a drive).
  pub dir_uri: Uri,
  pub kind: ConfigFileKind,
  /// Whether it's in a workspace folder, which a config file in an ancestor
  /// directory of the workspace folders or the global config file isn't.
  pub in_workspace: bool,
  /// Whether it was found on another machine and reading it isn't allowed
  /// yet, so the editor doesn't open or read it, which would reach the other
  /// machine. Its directory is watched like any other, which is never when
  /// it's a symlink that isn't a workspace folder or one of its ancestors (see
  /// "Lifecycle" in the editor extension docs). `dprint/configChanged` is sent
  /// once it's allowed, and when a config file becomes one on another machine
  /// (ex. a `git pull` replaced it with a symlink to a share), when its
  /// diagnostics are taken back.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub on_another_machine: bool,
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
  /// The config file with the directory it's used for, which is `None` when
  /// every config file may have changed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_file: Option<ConfigFileIdentifier>,
  /// How the config file changed, which is `None` without a `config_file`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub change: Option<ConfigChange>,
  /// Whether only its schema changed (ex. a plugin's schema downloaded).
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub schema_only: bool,
}

/// How a config file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigChange {
  Created,
  /// What it resolves to changed (ex. it was edited, a config file it extends
  /// changed, or something it needed was approved).
  Changed,
  Deleted,
}

/// Deserializes a field that may be `null` when it's provided, which is
/// `None` when it isn't.
fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
  D: serde::Deserializer<'de>,
  T: Deserialize<'de>,
{
  T::deserialize(deserializer).map(Some)
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
  fn should_serialize_empty_config_file_globs() {
    // `dprint lsp --config` has no config files to watch, which isn't the same as an older server that doesn't say
    let capability = ServerExperimentalCapability {
      version: Some(1),
      versions: vec![1],
      features: Default::default(),
      files: None,
      config_file_globs: Some(Vec::new()),
      config_file_names: Some(Vec::new()),
      dot_config_file_names: Some(Vec::new()),
      message: None,
    };
    assert_eq!(
      serde_json::to_value(&capability).unwrap(),
      json!({ "version": 1, "versions": [1], "configFileGlobs": [], "configFileNames": [], "dotConfigFileNames": [] })
    );
  }

  #[test]
  fn should_serialize_a_format_result_that_needs_approval() {
    let result = FormatResult::NotFormatted(NotFormatted::new(
      NotFormattedReason::ApprovalRequired,
      Some("message".to_string()),
      vec![PendingApproval {
        id: "1".to_string(),
        kind: ApprovalKind::ProcessPlugins,
        title: "Run process plugins".to_string(),
        config_file: ConfigFileInfo {
          uri: "file:///dprint.json".parse().unwrap(),
          dir_uri: "file:///".parse().unwrap(),
          kind: ConfigFileKind::Local,
          in_workspace: true,
          on_another_machine: false,
        },
        items: vec![ApprovalItem {
          specifier: "https://plugins.dprint.dev/exec.json@checksum".to_string(),
          kind: ApprovalItemKind::Plugin,
          registry: None,
          npmrc_uri: None,
          local_uri: None,
          plugin: Some(FilePlugin {
            name: "dprint-plugin-exec".to_string(),
            version: "0.5.0".to_string(),
            config_key: "exec".to_string(),
          }),
          configuration: Some(json!({ "commands": [] })),
        }],
        decisions: vec![ApprovalDecisionOption {
          decision: ApprovalDecision::Allow,
          label: "Allow".to_string(),
          allows: true,
          always: false,
          description: "description".to_string(),
        }],
        denied: false,
        previous_decision: None,
        message: "message".to_string(),
        trusted_configuration: None,
      }],
    ));
    let response = FormatResponse { config_file: None, result };
    assert_eq!(
      serde_json::to_value(&response).unwrap(),
      json!({
        "configFile": null,
        "result": {
          "kind": "notFormatted",
          "reason": "approvalRequired",
          "message": "message",
          "show": "command",
          "severity": "warning",
          "claim": true,
          "approvals": [{
            "id": "1",
            "kind": "processPlugins",
            "title": "Run process plugins",
            "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": true },
            "items": [{
              "specifier": "https://plugins.dprint.dev/exec.json@checksum",
              "kind": "plugin",
              "plugin": { "name": "dprint-plugin-exec", "version": "0.5.0", "configKey": "exec" },
              "configuration": { "commands": [] },
            }],
            "decisions": [{ "decision": "allow", "label": "Allow", "allows": true, "always": false, "description": "description" }],
            "denied": false,
            "message": "message",
          }],
        },
      })
    );
  }

  #[test]
  fn should_serialize_the_local_uri_of_a_network_path() {
    let item = ApprovalItem {
      specifier: r"C:\project\link -> \\server\share\dir".to_string(),
      kind: ApprovalItemKind::NetworkPath,
      registry: None,
      npmrc_uri: None,
      local_uri: Some("file:///C:/project/link".parse().unwrap()),
      plugin: None,
      configuration: None,
    };
    assert_eq!(
      serde_json::to_value(&item).unwrap(),
      json!({
        "specifier": r"C:\project\link -> \\server\share\dir",
        "kind": "networkPath",
        "localUri": "file:///C:/project/link",
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
        on_another_machine: false,
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
    assert_eq!(serde_json::to_value(FormatStatus::ready()).unwrap(), json!({ "kind": "ready" }));
    assert_eq!(
      serde_json::to_value(ApproveResult {
        config_approvals: ConfigApprovals {
          config_file: None,
          approvals: Vec::new(),
          status: FormatStatus::ready(),
        },
        notice: None,
      })
      .unwrap(),
      json!({ "configFile": null, "approvals": [], "status": { "kind": "ready" } })
    );
    let result = PendingApprovalsResult {
      approvals: Vec::new(),
      config_file: Some(None),
      status: Some(FormatStatus::ready()),
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value, json!({ "approvals": [], "configFile": null, "status": { "kind": "ready" } }));
    assert_eq!(serde_json::from_value::<PendingApprovalsResult>(value).unwrap(), result);
    assert_eq!(serde_json::to_value(PendingApprovalsResult::default()).unwrap(), json!({ "approvals": [] }));
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
    // `true` for a feature sent with options says that none of them are used
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
