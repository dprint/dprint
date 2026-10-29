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
/// The version of the `dprint/*` methods, which is increased when something is
/// added to them that a client needs to know about. A server supports the
/// versions before it, so a client checks that the server's is at least the one
/// it needs. A change that breaks a client uses a new capability name instead.
pub const EXPERIMENTAL_CAPABILITY_VERSION: u32 = 1;

pub const FORMAT_METHOD: &str = "dprint/format";
pub const FILE_INFO_METHOD: &str = "dprint/fileInfo";
pub const CONFIG_INFO_METHOD: &str = "dprint/configInfo";
pub const CONFIG_SCHEMA_METHOD: &str = "dprint/configSchema";
pub const APPROVE_METHOD: &str = "dprint/approve";
pub const RESET_METHOD: &str = "dprint/reset";
pub const PENDING_APPROVALS_METHOD: &str = "dprint/pendingApprovals";
pub const CONFIG_CHANGED_NOTIFICATION: &str = "dprint/configChanged";

/// The capability that the client and server provide in the experimental
/// capabilities, which is the version of the `dprint/*` methods they support.
/// The server only sends what was added at or below the client's version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalCapability {
  #[serde(default = "default_capability_version")]
  pub version: u32,
  /// Optional behavior that's opted into, where a feature that isn't known is
  /// ignored. There aren't any yet.
  #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
  pub features: serde_json::Map<String, serde_json::Value>,
}

impl Default for ExperimentalCapability {
  fn default() -> Self {
    Self {
      version: default_capability_version(),
      features: Default::default(),
    }
  }
}

fn default_capability_version() -> u32 {
  1
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
  /// The config to use instead of what's in the config file, which is only
  /// the editor's settings for the document (see `OVERRIDE_CONFIG_KEYS`).
  #[serde(default, skip_serializing_if = "ConfigKeyMap::is_empty")]
  pub override_config: ConfigKeyMap,
}

/// The keys `overrideConfig` may have, which are the editor's settings for
/// how a document is indented. Other config could have a plugin do something
/// else (ex. run a different command), which the user didn't approve.
pub const OVERRIDE_CONFIG_KEYS: [&str; 2] = ["indentWidth", "useTabs"];

/// The result of `dprint/format`.
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  /// What the user needs to approve to format it.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub approvals: Vec<PendingApproval>,
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
  /// What's downloaded for `download` or run for `processPlugins`.
  pub items: Vec<ApprovalItem>,
  /// The decisions the user can make in the order to show them.
  pub decisions: Vec<ApprovalDecision>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveResult {
  /// What needs to be approved now for the config file, which is empty only
  /// when nothing does. It has what was denied when the decision was to deny
  /// it, and otherwise what else needs to be approved (ex. a config file that a
  /// downloaded one extends or a process plugin the user denied before).
  pub approvals: Vec<PendingApproval>,
}

/// The data of the error for an approval that isn't pending anymore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalChangedErrorData {
  /// What needs to be approved for the config file now.
  pub approvals: Vec<PendingApproval>,
}

/// The params of `dprint/pendingApprovals`, which don't have anything yet.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalsParams {}

/// The result of `dprint/pendingApprovals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalsResult {
  /// What needs to be approved for the config files that were used, which
  /// includes what the user denied.
  pub approvals: Vec<PendingApproval>,
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
  /// The config file or a document that it's used for.
  pub uri: Uri,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory, which is what the setting says when `None`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub global_config: Option<GlobalConfigUse>,
  /// The token to report the progress of the request with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub work_done_token: Option<NumberOrString>,
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
  /// Globs of the files the plugins may format, which are relative to the
  /// directory of the config file (`dirUri`) and match the file extensions case
  /// insensitively. They're empty when the plugins aren't known.
  pub file_patterns: Vec<String>,
  /// The local files besides the config file that it was resolved from (ex.
  /// a config file it extends), which the editor watches for changes too.
  pub watch_uris: Vec<Uri>,
  pub status: FormatStatus,
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
  /// Whether it formats a range, which is `None` when it formats the lines of
  /// the range.
  pub supports_range_formatting: Option<bool>,
  pub help_url: String,
  /// Where its newer version is said to be.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub update_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileInfo {
  pub uri: Uri,
  /// The directory whose files it formats, which is the directory it's in
  /// unless that's a `.config` directory, and the directory it's used for when
  /// it's the global config file. It's `None` for a remote config file.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub dir_uri: Option<Uri>,
  pub kind: ConfigFileKind,
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
  Override,
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
  pub uri: Uri,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<ConfigChangeReason>,
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

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use super::*;

  #[test]
  fn should_serialize_a_format_result_that_needs_approval() {
    let result = FormatResult::NotFormatted(NotFormatted {
      reason: NotFormattedReason::ApprovalRequired,
      message: Some("message".to_string()),
      approvals: vec![PendingApproval {
        id: "1".to_string(),
        kind: ApprovalKind::ProcessPlugins,
        config_file: Some("file:///dprint.json".parse().unwrap()),
        items: vec![ApprovalItem {
          specifier: "https://plugins.dprint.dev/exec.json@checksum".to_string(),
          kind: Some(ApprovalItemKind::Plugin),
          name: Some("dprint-plugin-exec".to_string()),
          version: Some("0.5.0".to_string()),
          config_key: Some("exec".to_string()),
          configuration: Some(json!({ "commands": [] })),
        }],
        decisions: vec![ApprovalDecision::Allow, ApprovalDecision::Deny],
        denied: false,
        previous_decision: None,
        message: "message".to_string(),
      }],
    });
    assert_eq!(
      serde_json::to_value(&result).unwrap(),
      json!({
        "kind": "notFormatted",
        "reason": "approvalRequired",
        "message": "message",
        "approvals": [{
          "id": "1",
          "kind": "processPlugins",
          "configFile": "file:///dprint.json",
          "items": [{
            "specifier": "https://plugins.dprint.dev/exec.json@checksum",
            "kind": "plugin",
            "name": "dprint-plugin-exec",
            "version": "0.5.0",
            "configKey": "exec",
            "configuration": { "commands": [] },
          }],
          "decisions": ["allow", "deny"],
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
        dir_uri: Some("file:///a".parse().unwrap()),
        kind: ConfigFileKind::Local,
      }),
      plugins: None,
      formats_other_files: false,
      file_patterns: Vec::new(),
      watch_uris: Vec::new(),
      status: FormatStatus::NotFormatted(NotFormatted {
        reason: NotFormattedReason::ConfigError,
        message: Some("message".to_string()),
        approvals: Vec::new(),
      }),
    };
    assert_eq!(
      serde_json::to_value(&result).unwrap(),
      json!({
        "configFile": { "uri": "file:///a/.config/dprint.json", "dirUri": "file:///a", "kind": "local" },
        "plugins": null,
        "formatsOtherFiles": false,
        "filePatterns": [],
        "watchUris": [],
        "status": { "kind": "notFormatted", "reason": "configError", "message": "message" },
      })
    );
    assert_eq!(serde_json::to_value(FormatStatus::Ready).unwrap(), json!({ "kind": "ready" }));
    assert_eq!(
      serde_json::to_value(ApproveResult { approvals: Vec::new() }).unwrap(),
      json!({ "approvals": [] })
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
    assert_eq!(
      serde_json::from_value::<ConfigParams>(json!({ "uri": "file:///dprint.json" }))
        .unwrap()
        .global_config,
      None
    );
    assert_eq!(
      serde_json::from_value::<ConfigParams>(json!({ "uri": "file:///dprint.json", "globalConfig": "never" }))
        .unwrap()
        .global_config,
      Some(GlobalConfigUse::Never)
    );
    assert_eq!(serde_json::from_value::<Option<ResetParams>>(json!({})).unwrap(), Some(ResetParams {}));
  }

  #[test]
  fn should_resolve_when_the_global_config_file_is_used() {
    assert!(GlobalConfigUse::resolve(None, true));
    assert!(!GlobalConfigUse::resolve(None, false));
    assert!(GlobalConfigUse::resolve(Some(GlobalConfigUse::WhenNoConfigFile), false));
    assert!(!GlobalConfigUse::resolve(Some(GlobalConfigUse::Never), true));
  }
}
