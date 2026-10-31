//! Formats the files of a whole workspace by finding the config file of each
//! file, which the language server uses for both its text document methods and
//! the `dprint/*` methods that an editor extension uses.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;

use anyhow::Result;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use indexmap::IndexMap;
use tokio_util::sync::CancellationToken;

use crate::commands::lsp::client::ClientWrapper;
use crate::commands::lsp::config_completion::ConfigJsonSchemaBuilder;
use crate::commands::lsp::config_completion::PluginSchemaUrl;
use crate::commands::lsp::file_path_to_uri;
use crate::commands::lsp::language::get_language_file_name;
use crate::commands::lsp::notebook::trim_formatted_cell_text;
use crate::commands::lsp::text::RangeFormat;
use crate::commands::lsp::text::Utf16TextEdit;
use crate::commands::lsp::text::apply_utf16_edits;
use crate::commands::lsp::text::get_range_utf16_edits;
use crate::commands::lsp::text::get_utf16_edits;
use crate::commands::lsp::text::map_utf16_range;
use crate::commands::lsp::text::normalize_to_source_line_endings;
use crate::commands::lsp::untitled::is_untitled_uri;
use crate::commands::lsp::uri_to_file_path;
use crate::commands::no_config::DISMISS_ACTION_TITLE;
use crate::commands::no_config::DISMISS_WORKSPACE_ACTION_TITLE;
use crate::commands::no_config::EDITOR_EXTENSION_SETTINGS_OWNER;
use crate::commands::no_config::LANGUAGE_SERVER_SETTINGS_OWNER;
use crate::commands::no_config::NoConfigMessageOptions;
use crate::commands::no_config::NoConfigNotificationDismissal;
use crate::commands::no_config::dismiss_no_config_notification;
use crate::commands::no_config::get_no_config_message;
use crate::commands::no_config::is_no_config_notification_dismissed;
use crate::commands::not_formatted_messages::NO_CELL_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NO_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NOT_MATCHED_MESSAGE;
use crate::commands::not_formatted_messages::RANGE_FORMATTING_NOT_SUPPORTED_MESSAGE;
use crate::commands::process_plugin_approval::PROCESS_PLUGIN_ACTION_TITLES;
use crate::commands::process_plugin_approval::get_process_plugin_action_title;
use crate::commands::process_plugin_approval::get_process_plugin_decision;
use crate::commands::process_plugin_approval::get_process_plugin_store_error_message;
use crate::commands::process_plugin_approval::store_process_plugin_action;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::commands::scope_container::get_config_file_key;
use crate::configuration::ConfigFileOnAnotherMachineError;
use crate::configuration::ConfigMapValue;
use crate::configuration::NewerApprovalsFileError;
use crate::configuration::POSSIBLE_CONFIG_FILE_NAMES;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::SessionPermissions;
use crate::configuration::ensure_allows_reading_config_file;
use crate::configuration::forget_editor_approvals;
use crate::configuration::get_config_file_found_path;
use crate::configuration::get_config_file_lookup;
use crate::configuration::get_editor_approvals_file_path;
use crate::configuration::get_settings_file_path;
use crate::configuration::is_dot_config_dir;
use crate::configuration::is_more_recent_at;
use crate::configuration::read_editor_approval_denials;
use crate::configuration::read_editor_approvals;
use crate::configuration::read_ignored_settings_message;
use crate::configuration::resolve_global_config_dir;
use crate::configuration::resolve_global_config_path_and_text;
use crate::configuration::resolve_global_config_path_and_text_at_path_root;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::paths::get_plugin_names_for_file_on_disk;
use crate::patterns::OrderedPatternsMatcher;
use crate::patterns::to_dir_relative_globs;
use crate::plugins::PluginConfigDiagnostic;
use crate::resolution::GetPluginResult;
use crate::resolution::PluginConfigDiagnosticsError;
use crate::resolution::PluginsScope;
use crate::resolution::RangeFormattingSupport;
use crate::resolution::UnapprovedProcessPlugin;
use crate::resolution::UnapprovedProcessPlugins;
use crate::resolution::get_cant_ask_message;
use crate::utils::CANCEL_DOWNLOAD_ACTION_TITLE;
use crate::utils::DOWNLOAD_ACTION_TITLE;
use crate::utils::LogLevel;
use crate::utils::NpmrcProxy;
use crate::utils::PathSource;
use crate::utils::Semaphore;
use crate::utils::UntrustedDownloadError;
use crate::utils::escape_text;
use crate::utils::get_bytes_hash;
use crate::utils::get_download_url_text;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;
use crate::utils::get_npm_registry_download_url;
use crate::utils::get_path_download_local_path;
use crate::utils::get_sha256_checksum;
use crate::utils::get_untrusted_download_message;
use crate::utils::is_combining_mark;
use crate::utils::is_escaped_in_text;
use crate::utils::is_path_download;
use crate::utils::normalize_download;

mod config_diagnostics;
mod editor_paths;
pub mod messages;
mod session_file;

use self::config_diagnostics::ConfigDiagnostic;
use self::config_diagnostics::PluginSectionDiagnostic;
use self::config_diagnostics::get_config_diagnostics;
use self::config_diagnostics::get_config_error_diagnostic;
use self::editor_paths::EditorPaths;
use self::messages::Action;
use self::messages::ApprovalDecision;
use self::messages::ApprovalDecisionOption;
use self::messages::ApprovalItem;
use self::messages::ApprovalItemKind;
use self::messages::ApprovalKind;
use self::messages::ApproveResult;
use self::messages::ConfigApprovals;
use self::messages::ConfigChange;
use self::messages::ConfigChangedParams;
use self::messages::ConfigFileIdentifier;
use self::messages::ConfigFileInfo;
use self::messages::ConfigFileKind;
use self::messages::ConfigInfoResult;
use self::messages::Decision;
use self::messages::DocumentLanguage;
use self::messages::FileInfoResult;
use self::messages::FilePattern;
use self::messages::FilePlugin;
use self::messages::FormatFailedReason;
use self::messages::FormatStatus;
use self::messages::GlobalConfigUse;
use self::messages::NotFormatted;
use self::messages::NotFormattedReason;
use self::messages::Notice;
use self::messages::OtherFiles;
use self::messages::OtherFilesKind;
use self::messages::PendingApproval;
use self::messages::PluginInfo;
use self::messages::RangeFormatting;
use self::messages::Severity;
use self::messages::ShowPolicy;
use self::session_file::AllowedDownload;
use self::session_file::DeniedDownload;
use self::session_file::SESSION_FILE_CHECK_INTERVAL;
use self::session_file::SESSION_LOCK_RETRY_INTERVAL;
use self::session_file::SessionFile;
use self::session_file::SessionFileWriteError;
use self::session_file::SessionLockError;
use self::session_file::SessionState;
pub(crate) use self::session_file::get_sessions_dir;
use self::session_file::remove_stale_session_files;
pub(crate) use self::session_file::remove_unused_session_files;

/// What the user is told when they denied downloads that were each allowed
/// exactly since (ex. for another config file, or for the same config file
/// once it changed), which is still allowed then. A new session asks about it
/// again, apart from what was downloaded to the cache, which isn't downloaded
/// again.
const ALLOWED_SINCE_MESSAGE: &str = "What you denied was allowed since, so it's still allowed for this session. Restarting dprint in a new session asks about it again, unless it was downloaded to dprint's cache, which `dprint clear-cache` removes.";

/// What's said of a path on another machine that isn't valid Unicode, which
/// can't be allowed (see `TrustedDownloads::is_path_allowed`).
const NOT_ALLOWABLE_PATH_MESSAGE: &str =
  "It can't be allowed because its path isn't valid Unicode, so it can't be told apart from other paths. Rename it to use it.";

/// The error for approving what isn't pending anymore (ex. what needs to be
/// approved changed because the config file changed).
#[derive(Debug, thiserror::Error)]
#[error("Nothing needs to be approved with id {id}, which may be because what needs to be approved changed.")]
pub struct ApprovalChangedError {
  id: String,
  /// The config file the approval was for and what needs to be approved for
  /// it now when it's known.
  pub current: Option<ConfigApprovals>,
}

/// The error for a document that can't be formatted, which is a reason it
/// isn't formatted instead of an error for the methods that say why.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct UnsupportedDocumentError {
  reason: NotFormattedReason,
  message: String,
}

impl UnsupportedDocumentError {
  fn unsupported(message: String) -> anyhow::Error {
    UnsupportedDocumentError {
      reason: NotFormattedReason::UnsupportedDocument,
      message,
    }
    .into()
  }
}

/// Gets why a document isn't formatted when the error is for a document that
/// can't be formatted, which is otherwise the error.
fn into_unsupported_document(err: anyhow::Error) -> Result<NotFormatted> {
  match err.downcast::<UnsupportedDocumentError>() {
    Ok(err) => Ok(NotFormatted::new(err.reason, Some(err.message), Vec::new())),
    Err(err) => Err(err),
  }
}

/// The error for params that can't be handled (ex. a document that isn't a
/// file or a decision that an approval doesn't have).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidParamsError(pub String);

/// How often to check whether dprint's own files changed when they can't be
/// watched (see `Workspace::watch_dprint_files`).
const DPRINT_FILES_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
/// How long to wait for the other events of a change to dprint's own files.
const DPRINT_FILES_CHANGE_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

/// The files a config file's plugins may format and the other files it formats.
type FilePatterns = (Vec<FilePattern>, Vec<OtherFiles>);

/// What a config file is resolved for.
pub enum ConfigTarget {
  /// The config file itself with the directory the global config file is
  /// used for (see `ConfigFileIdentifier`).
  ConfigFile { uri: Uri, dir_uri: Option<Uri> },
  /// A document it's used for.
  Document(DocumentInfo),
}

/// How many of the files of the approvals that were provided to keep.
const MAX_APPROVAL_FILES: usize = 256;

/// How long to wait for more config files to change before handling the ones
/// that did.
const CONFIG_CHANGE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// How a config file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileChange {
  Created,
  Changed,
  Deleted,
}

/// A file the editor said changed, which is a config file or a file a config
/// file was resolved from (see `ConfigInfoResult::watch_uris`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedFileChange {
  pub file_path: PathBuf,
  pub change: ConfigFileChange,
  pub is_config_file: bool,
}

/// How the client works with the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
  /// A language client, which only uses the text document methods, so the
  /// server asks the user what needs to be approved and notifies the user
  /// about a file without a config file.
  LanguageClient {
    /// Whether the client shows the actions of a `window/showMessageRequest`.
    supports_message_actions: bool,
  },
  /// An editor extension, which uses the `dprint/*` methods and asks the
  /// user what needs to be approved itself (see `PendingApproval`).
  EditorExtension,
}

/// The settings of the workspace, which the client provides.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceSettings {
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory.
  pub use_global_config: bool,
  /// Whether to notify a language client's user the first time a file isn't
  /// formatted because there's no config file for it.
  pub show_no_config_notification: bool,
  pub ensure_stable_format: Option<bool>,
  /// Whether to log debug messages, which is the log level dprint was started
  /// with when `None`.
  pub verbose: Option<bool>,
  /// The paths of the workspace folders.
  pub workspace_folders: Vec<PathBuf>,
}

/// What identifies a document, which is formatted as a file.
#[derive(Debug, Clone)]
pub struct DocumentInfo {
  pub uri: Uri,
  /// The language of the document, which is needed for a document that isn't
  /// a file (ex. an untitled document or a notebook cell).
  pub language: Option<DocumentLanguage>,
  /// The uri of the notebook when the document is one of its cells.
  pub notebook_uri: Option<Uri>,
  /// The file to format the document as instead of its uri (see
  /// `DocumentParams::file_uri`).
  pub file_uri: Option<Uri>,
}

/// What to format.
pub struct FormatRequest {
  pub document: DocumentInfo,
  pub text: String,
  /// The ranges of UTF-16 offsets in the text to format, which is the whole
  /// text when `None`.
  pub ranges: Option<Vec<Range<u32>>>,
  pub override_config: ConfigKeyMap,
  pub global_config: Option<GlobalConfigUse>,
  /// Whether to notify a language client's user when there's no config file
  /// for the document, which isn't for a command that says why itself.
  pub notify_no_config: bool,
}

/// The outcome of formatting a document.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum FormatOutcome {
  /// The edits of the text, which are empty when it's already formatted.
  Formatted(Vec<Utf16TextEdit>),
  NotFormatted(NotFormatted),
  Failed {
    reason: FormatFailedReason,
    message: String,
    plugin: Option<FilePlugin>,
  },
}

/// The workspace that the language server formats the files of.
pub struct Workspace<TEnvironment: Environment> {
  environment: TEnvironment,
  client: ClientWrapper,
  scope_container: Rc<PluginsScopeContainer<TEnvironment>>,
  concurrency_limiter: Rc<Semaphore>,
  ensure_stable_format: EnsureStableFormat,
  config_schema_builder: ConfigJsonSchemaBuilder<TEnvironment>,
  /// The log level dprint was started with, which the `verbose` setting changes.
  initial_log_level: LogLevel,
  client_kind: Cell<ClientKind>,
  settings: RefCell<WorkspaceSettings>,
  /// What the user allowed for this session.
  session_permissions: Arc<SessionPermissions>,
  /// The error that was shown to the user for resolving each config file,
  /// which is removed once it resolves.
  config_errors: RefCell<HashMap<String, String>>,
  /// The error finding a config file for each directory, which is removed
  /// once one is found. An error is only shown when no other directory has
  /// it because it's often for an ancestor directory (ex. a config file that
  /// can't be read).
  config_find_errors: RefCell<HashMap<PathBuf, String>>,
  /// The error that was shown to the user for the plugin config diagnostics
  /// of each config file.
  plugin_config_errors: RefCell<HashMap<String, ShownPluginConfigError<TEnvironment>>>,
  /// What each config file last resolved to, which is how a change to a config
  /// file it extends or to what needs to be approved (ex. outside the editor)
  /// is noticed.
  resolved_configs: RefCell<HashMap<String, ResolvedConfigState>>,
  /// The scope whose config diagnostics were checked for each config file.
  checked_scopes: RefCell<HashMap<String, CheckedScope<TEnvironment>>>,
  /// The config diagnostics that were last published for each config file uri.
  /// They're by the canonicalized uri with the editor's uri they were published to.
  published_diagnostics: RefCell<HashMap<String, (Uri, Vec<ConfigDiagnostic>)>>,
  /// The plugins of each config file uri the last time they were known, which
  /// are provided when it can't be used (ex. it has an error) so that the
  /// editor doesn't change what it formats with it.
  last_plugin_infos: RefCell<HashMap<String, LastPluginInfos>>,
  /// What needs to be approved for each config file, which is what a client
  /// approves by its id.
  approvals: RefCell<HashMap<String, Vec<StoredApproval>>>,
  /// A file that each approval that was provided is for, which says what
  /// needs to be approved now for an approval that isn't pending anymore, with
  /// the downloads of one for downloads (see `approve`). Only the most recent
  /// ones are kept.
  approval_files: RefCell<IndexMap<String, ProvidedApproval>>,
  /// The config files that were used by their key, which are resolved again
  /// when dprint's own files change.
  used_configs: RefCell<HashMap<String, ResolvedConfigPathWithText>>,
  /// The keys of the config files that failed to resolve, which tells the
  /// editor extension once they resolve again.
  failed_configs: RefCell<HashSet<String>>,
  /// What each config file uri was resolved from the last time it resolved,
  /// which is kept when it fails to resolve so that fixing one of them
  /// resolves it again.
  config_dependencies: RefCell<HashMap<String, ConfigDependencies>>,
  /// The config file uris whose plugin schemas are being waited on.
  schema_waits: RefCell<HashSet<String>>,
  /// Stops what's done in the background once the server shuts down.
  stopped: tokio_util::sync::CancellationToken,
  /// The file patterns of each config file uri the last time they were known
  /// (see `last_plugin_infos`).
  last_file_patterns: RefCell<HashMap<String, FilePatterns>>,
  /// The downloads the user denied this session by their approval id.
  denied_downloads: RefCell<HashMap<String, DeniedDownload>>,
  /// The items of the downloads the user allowed this session by what's
  /// allowed (see `UntrustedDownloadError`), which is what the decisions show.
  allowed_download_items: RefCell<HashMap<String, ApprovalItem>>,
  /// The paths of the directories the editor knows by another path than their
  /// canonical one, which are the paths the server provides.
  editor_paths: EditorPaths,
  /// The canonical directories of `editor_paths` whose alias was checked for
  /// the request being handled (see `editor_path`).
  checked_editor_dirs: RefCell<HashSet<PathBuf>>,
  /// The ids of the approvals that a language client's user was asked about
  /// this session.
  prompted_approvals: RefCell<HashSet<String>>,
  /// Whether a language client's user was notified this session about a file
  /// without a config file.
  has_notified_no_config: Cell<bool>,
  /// The files the editor said changed that weren't handled yet.
  pending_config_changes: RefCell<Vec<WatchedFileChange>>,
  /// Whether the client provided the settings, which changing tells an
  /// editor extension about.
  has_settings: Cell<bool>,
  /// The canonicalized paths of the workspace folders, which the paths of
  /// config files are compared to.
  canonical_workspace_folders: RefCell<Vec<PathBuf>>,
  /// Whether there was a global config file the last time dprint's own files
  /// were checked, which creating tells an editor extension about.
  had_global_config: Cell<bool>,
  /// The files outside the workspace that the client was asked to watch for
  /// the `watchFiles` feature, which is `None` when it isn't used.
  registered_watch_files: RefCell<Option<Vec<PathBuf>>>,
  /// The file that keeps what the user decided for the session when the
  /// client provided a session id, so a server started again for the same
  /// session uses it.
  session_file: RefCell<Option<Rc<SessionFile>>>,
}

/// The plugins of a config file the last time they were known.
struct LastPluginInfos {
  /// What identifies the plugins of the config file they're for, which they
  /// aren't for anymore once a plugin was added or changed.
  plugin_references: Vec<String>,
  plugins: Vec<PluginInfo>,
  /// The config they were for, which says what they may format while the
  /// config file has an error (see `get_failure_not_formatted`).
  config: Rc<ResolvedConfig>,
}

/// What a config file resolved to.
#[derive(Clone, PartialEq)]
struct ResolvedConfigState {
  config: Rc<ResolvedConfig>,
  untrusted_downloads: Vec<String>,
  process_plugins_allowed: bool,
  /// Whether the download and the process plugins that need to be approved
  /// were denied, which changes whether the editor extension keeps acting as
  /// the formatter of its documents.
  denied: (bool, bool),
}

/// What needs to be approved for a config file.
/// An approval that was provided (see `Workspace::approval_files`).
#[derive(Clone)]
struct ProvidedApproval {
  /// What its config file is resolved for.
  target: ApprovalTarget,
  /// What it's for when it's for downloads.
  downloads: Option<Vec<String>>,
}

#[derive(Clone)]
struct StoredApproval {
  /// The key of the config file it's for (see `get_config_key`).
  config_key: String,
  /// What the config file it's for is resolved again for once it's approved,
  /// which is the config file itself by the path that identifies it unless it
  /// can't be (ex. a remote one) and then a file it was used for.
  config_target: ApprovalTarget,
  approval: PendingApproval,
  subject: ApprovalSubject,
}

/// What a config file that needs something approved is resolved for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ApprovalTarget {
  /// A file the config file is used for.
  File { file_path: PathBuf, use_global_config: bool },
  /// The config file itself (ex. one that `dprint/configInfo` was asked
  /// about) with the directory the global config file is used for.
  ConfigFile { file_path: PathBuf, dir_path: Option<PathBuf> },
}

/// What's approved.
#[derive(Clone)]
enum ApprovalSubject {
  Downloads(Vec<String>),
  ProcessPlugins(UnapprovedProcessPlugins),
}

/// The config file that's used for a document.
#[allow(clippy::large_enum_variant)]
enum DocumentConfig<TEnvironment: Environment> {
  Resolved(Rc<PluginsScope<TEnvironment>>),
  /// There's no config file for the document.
  NotFound,
  /// The config file failed to resolve.
  Failed(ConfigFailure),
}

/// A config file that failed to resolve.
struct ConfigFailure {
  /// The config file, which is `None` when it failed to be found (ex. an
  /// ancestor directory couldn't be read).
  config_file: Option<ConfigFileInfo>,
  /// The key of the config file (see `get_config_key`) and the uri that
  /// identifies it when it's known (see `ResolvedConfig::file_path`).
  config_key: Option<(String, Uri)>,
  /// What the config file is resolved for by the path that identifies it when it can be.
  config_target: Option<ApprovalTarget>,
  /// Why it failed, which is `ConfigError` unless what it extends needs to be
  /// downloaded (`ApprovalRequired`).
  reason: NotFormattedReason,
  message: String,
  /// What needs to be downloaded that the download policy doesn't allow
  /// without asking the user.
  downloads: Vec<String>,
}

/// The files a document may be formatted as.
struct DocumentFile {
  /// The file paths in order of preference, which are in the same directory
  /// and never empty.
  candidates: Vec<PathBuf>,
  /// Whether the candidate is the path of the document itself, which the
  /// editor opened, rather than a file it's formatted as (ex. a `fileUri` or
  /// the file of an untitled document), which may not exist.
  is_document_path: bool,
  /// The notebook's path when the document is one of its cells.
  notebook_path: Option<PathBuf>,
}

/// The file a document is formatted as.
struct ChosenFile {
  file_path: PathBuf,
  /// Why it can't be formatted, which is `None` when it can.
  cant_format_reason: Option<NotFormattedReason>,
}

/// A scope whose config diagnostics were checked.
struct CheckedScope<TEnvironment: Environment> {
  scope: Weak<PluginsScope<TEnvironment>>,
  /// The text of the config file, which is where the diagnostics are.
  config_text: Rc<str>,
}

/// The error that was shown to the user for the plugin config diagnostics of
/// a config file.
struct ShownPluginConfigError<TEnvironment: Environment> {
  /// The scope that had the diagnostics, so they're shown again once the
  /// config file changes.
  scope: Weak<PluginsScope<TEnvironment>>,
  message: String,
}

impl<TEnvironment: Environment> Workspace<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    client: ClientWrapper,
    scope_container: Rc<PluginsScopeContainer<TEnvironment>>,
    concurrency_limiter: Rc<Semaphore>,
    session_permissions: Arc<SessionPermissions>,
  ) -> Self {
    Self {
      ensure_stable_format: EnsureStableFormat::for_editor(&environment),
      config_schema_builder: ConfigJsonSchemaBuilder::new(environment.clone()),
      initial_log_level: environment.log_level(),
      client_kind: Cell::new(ClientKind::LanguageClient {
        supports_message_actions: false,
      }),
      settings: Default::default(),
      config_errors: Default::default(),
      config_find_errors: Default::default(),
      plugin_config_errors: Default::default(),
      resolved_configs: Default::default(),
      checked_scopes: Default::default(),
      published_diagnostics: Default::default(),
      last_plugin_infos: Default::default(),
      approvals: Default::default(),
      approval_files: Default::default(),
      used_configs: Default::default(),
      failed_configs: Default::default(),
      config_dependencies: Default::default(),
      schema_waits: Default::default(),
      stopped: Default::default(),
      last_file_patterns: Default::default(),
      denied_downloads: Default::default(),
      allowed_download_items: Default::default(),
      editor_paths: Default::default(),
      checked_editor_dirs: Default::default(),
      prompted_approvals: Default::default(),
      has_notified_no_config: Default::default(),
      pending_config_changes: Default::default(),
      has_settings: Default::default(),
      canonical_workspace_folders: Default::default(),
      had_global_config: Default::default(),
      registered_watch_files: Default::default(),
      session_file: Default::default(),
      environment,
      client,
      scope_container,
      concurrency_limiter,
      session_permissions,
    }
  }

  pub fn set_client_kind(self: &Rc<Self>, client_kind: ClientKind) {
    self.client_kind.set(client_kind);
    self.watch_dprint_files();
  }

  /// Keeps what the user decides for the session in the session file of the
  /// session id the client provided, which has what was decided before when a
  /// server of the same session stopped without the session ending (ex. it
  /// crashed). It's only used when the id is valid, which is logged
  /// otherwise.
  ///
  /// It's only used once the server holds the session's lock. When another
  /// server that's running holds it, or taking it fails, that's logged, what
  /// the other server kept isn't used, and the lock is tried again until it's
  /// taken (see `keep_session_file`). The session files that weren't written
  /// for a while are deleted too.
  pub fn use_session(self: &Rc<Self>, session_id: Option<&serde_json::Value>) {
    let session_file = session_id.and_then(|session_id| {
      let session_file = session_id.as_str().and_then(|session_id| SessionFile::new(&self.environment, session_id));
      if session_file.is_none() {
        log_warn!(
          self.environment,
          "Ignoring the session id {} because it isn't a string of 16 to 128 letters, digits, underscores, and hyphens, so what's decided for the session is lost when the server stops.",
          session_id
        );
      }
      session_file.map(|session_file| (session_id, session_file))
    });
    if let Some((session_id, session_file)) = session_file {
      let session_file = Rc::new(session_file);
      match session_file.lock(&self.environment) {
        Ok(()) => {
          if let Some(state) = session_file.read(&self.environment) {
            self.restore_session_state(state);
            // so it isn't stale
            if let Err(err) = session_file.write(&self.environment, &self.get_session_state(), true) {
              log_warn!(self.environment, "{}", err);
            }
          }
        }
        Err(err) => {
          let reason = match err {
            SessionLockError::InUse => "another dprint language server that's running holds it".to_string(),
            SessionLockError::Failed(err) => format!("{:#}", err),
          };
          log_warn!(
            self.environment,
            "Not keeping what's decided for the session {} for now because its lock {} couldn't be taken ({}), so if the server stops before this server takes the lock, which it keeps trying, it's lost and a server started again for the session may use what the server that held the lock kept instead. The editor extension needs to give each server its own session id, and start a server again for a session only once the previous one exited.",
            session_id,
            session_file.lock_path().display(),
            reason
          );
        }
      }
      *self.session_file.borrow_mut() = Some(session_file);
      self.keep_session_file();
    }
    remove_stale_session_files(&self.environment);
  }

  /// Has the client be asked to watch the files that config files were
  /// resolved from which are outside the workspace (the `watchFiles` feature).
  pub fn use_watch_files_registration(&self) {
    *self.registered_watch_files.borrow_mut() = Some(Vec::new());
  }

  pub fn set_settings(&self, settings: WorkspaceSettings) {
    self.environment.set_log_level(match settings.verbose {
      Some(true) => LogLevel::Debug,
      Some(false) if self.initial_log_level == LogLevel::Debug => LogLevel::Info,
      _ => self.initial_log_level,
    });
    *self.canonical_workspace_folders.borrow_mut() = settings.workspace_folders.iter().map(|folder| self.canonicalize(folder.clone())).collect();
    let previous = std::mem::replace(&mut *self.settings.borrow_mut(), settings);
    // the documents without a config file and the untitled documents may be formatted differently now
    let settings = self.settings.borrow();
    if self.has_settings.replace(true) && (previous.use_global_config != settings.use_global_config || previous.workspace_folders != settings.workspace_folders)
    {
      self.send_config_changed(new_every_config_changed());
    }
  }

  /// Forgets the config files and stops the plugins, which are set up again
  /// when they're next used (ex. a plugin that stopped responding or changed).
  /// What the user decided for the session is kept.
  pub async fn reset(&self) {
    log_debug!(self.environment, "Resetting the configuration files and plugins.");
    self.used_configs.borrow_mut().clear();
    self.scope_container.shutdown().await;
  }

  /// Stops the plugins once the client asked the server to shut down. The
  /// session file isn't written anymore, but it's only deleted once the
  /// session ends (see `end_session`), so a client that kills the server
  /// after this (ex. one that stopped responding) keeps the session.
  pub async fn shutdown(&self) {
    self.stopped.cancel();
    if let Some(session_file) = &*self.session_file.borrow() {
      session_file.close();
    }
    self.scope_container.shutdown().await;
  }

  /// Ends the session once the client exited the server after shutting it
  /// down, which deletes its session file.
  pub fn end_session(&self) {
    if let Some(session_file) = &*self.session_file.borrow() {
      session_file.delete(&self.environment);
    }
  }

  /// Handles the config files the editor says were created, changed, or
  /// deleted, which is done together after a short delay because several
  /// often change at once (ex. switching branches). Otherwise, a config file is
  /// only read again by the next request that uses it.
  pub fn config_files_changed(self: &Rc<Self>, changes: Vec<WatchedFileChange>) {
    let mut pending_changes = self.pending_config_changes.borrow_mut();
    let was_empty = pending_changes.is_empty();
    for change in changes {
      // the most recent change of a file is what's handled
      pending_changes.retain(|pending| pending.file_path != change.file_path);
      pending_changes.push(change);
    }
    if !was_empty {
      return;
    }
    let workspace = self.clone();
    dprint_core::async_runtime::spawn(async move {
      tokio::time::sleep(CONFIG_CHANGE_DELAY).await;
      workspace.check_editor_dirs_again();
      let changes = std::mem::take(&mut *workspace.pending_config_changes.borrow_mut());
      for change in changes {
        // a config file that was resolved from the file resolves to something else now, which includes
        // one whose canonical path is the file (ex. a `dprint.json` that's a symlink to a `base.json`)
        for (config_file_path, is_canonical_path) in workspace.get_dependent_config_files(&change.file_path) {
          // the config file is a dangling symlink now, which is like one that was deleted, unless it points
          // elsewhere now (ex. a checkout changed the symlink and deleted what it pointed to)
          let is_deleted = is_canonical_path
            && change.change == ConfigFileChange::Deleted
            && workspace.allows_reading_path(&config_file_path)
            && !workspace.environment.path_exists(&config_file_path);
          let config_file_change = match is_deleted {
            true => ConfigFileChange::Deleted,
            false => ConfigFileChange::Changed,
          };
          let key = file_path_to_uri(&config_file_path).map(|uri| uri.as_str().to_string()).unwrap_or_default();
          let dependencies = workspace.config_dependencies.borrow().get(&key).cloned();
          workspace.handle_config_file_change(&config_file_path, config_file_change).await;
          // it's still watched by what it points to, so the editor reports that being created again
          if is_deleted && let Some(dependencies) = dependencies {
            workspace.config_dependencies.borrow_mut().insert(key, dependencies);
            workspace.register_watch_files();
          }
        }
        // the config file the server was started with is the only one
        if workspace.scope_container.has_config_override() {
          if workspace.is_config_override_file(&change.file_path) {
            workspace.handle_config_override_change(&change.file_path, change.change).await;
          }
        } else if change.is_config_file {
          workspace.handle_config_file_change(&change.file_path, change.change).await;
        }
      }
    });
  }

  /// Resolves a config file that changed again when it was resolved before,
  /// which publishes its diagnostics and tells an editor extension when what it
  /// formats changed. A config file that wasn't resolved before isn't because
  /// nothing asked for it, which would download and set up its plugins (an
  /// editor extension is told it changed instead).
  async fn handle_config_file_change(self: &Rc<Self>, file_path: &Path, change: ConfigFileChange) {
    // the config files are known by their paths in the directories they're used for, and one that's on
    // another machine now (ex. a `git pull` replaced it with a symlink to a share) is asked about like any other
    let file_path = &self.canonicalize_config_file_path(file_path.to_path_buf());
    // the server watches the global config file itself (see `watch_dprint_files`)
    if self.get_global_config_path().as_deref() == Some(file_path.as_path()) {
      return;
    }
    let Some(uri) = file_path_to_uri(file_path) else {
      return;
    };
    match change {
      ConfigFileChange::Changed if self.was_resolved(file_path) => {
        // an editor extension is told when what it resolves to changed, and what
        // needs to be approved may have changed too (ex. a process plugin's
        // configuration), which is for this file even when its directory uses another
        let target = ApprovalTarget::ConfigFile {
          file_path: file_path.to_path_buf(),
          dir_path: None,
        };
        self.get_approvals_for_target(&target).await;
      }
      ConfigFileChange::Deleted => {
        // a deleted config file doesn't have problems or need anything approved anymore
        if let Some(editor_uri) = self.editor_uri(file_path) {
          self.publish_diagnostics(&uri, editor_uri, Vec::new());
        }
        self.forget_config_file_approvals(&uri);
        self.forget_config_file(file_path);
        self.send_file_config_changed(file_path, ConfigChange::Deleted);
      }
      ConfigFileChange::Created => self.send_file_config_changed(file_path, ConfigChange::Created),
      ConfigFileChange::Changed => self.send_file_config_changed(file_path, ConfigChange::Changed),
    }
  }

  /// Resolves what the config file the server was started with is used for
  /// again after it changed, which publishes its diagnostics, and tells an
  /// editor extension that every config file may have changed because it's
  /// used for every file.
  async fn handle_config_override_change(self: &Rc<Self>, file_path: &Path, change: ConfigFileChange) {
    let file_path = self.canonicalize_config_file_path(file_path.to_path_buf());
    if change == ConfigFileChange::Deleted
      && let (Some(uri), Some(editor_uri)) = (file_path_to_uri(&file_path), self.editor_uri(&file_path))
    {
      self.publish_diagnostics(&uri, editor_uri, Vec::new());
    }
    self.resolve_used_configs_again().await;
    self.send_config_changed(new_every_config_changed());
  }

  /// Tells an editor extension that a local config file the editor said
  /// changed did.
  fn send_file_config_changed(&self, file_path: &Path, change: ConfigChange) {
    if let Some(params) = self.new_file_config_changed(file_path, change) {
      self.send_config_changed(params);
    }
  }

  /// Gets the config files that were resolved from the file (ex. it's a config
  /// file they extend), with whether it's the canonical path of the config file.
  /// A config file that's a symlink is matched by where it was found too (ex. a
  /// config file that inherits from it, which a changed symlink changes).
  fn get_dependent_config_files(&self, file_path: &Path) -> Vec<(PathBuf, bool)> {
    let canonical_path = self.canonicalize_editor_path(file_path.to_path_buf());
    let found_path = self.get_config_file_identity_path(file_path, canonical_path.clone());
    let file_path = canonical_path;
    let mut config_file_paths: Vec<(PathBuf, bool)> = Vec::new();
    for dependencies in self.config_dependencies.borrow().values() {
      let is_canonical_path = dependencies.canonical_path.as_ref() == Some(&file_path);
      let is_dependency = dependencies.dependencies.contains(&file_path) || dependencies.dependencies.contains(&found_path);
      if (is_canonical_path || is_dependency) && !config_file_paths.iter().any(|(path, _)| *path == dependencies.file_path) {
        config_file_paths.push((dependencies.file_path.clone(), is_canonical_path));
      }
    }
    config_file_paths
  }

  /// Gets whether the config file at the path that identifies it (see
  /// `canonicalize_config_file_path`) was resolved before.
  fn was_resolved(&self, file_path: &Path) -> bool {
    // like `forget_config_file`, which compares the paths rather than their text
    self
      .resolved_configs
      .borrow()
      .keys()
      .chain(self.config_errors.borrow().keys())
      .chain(self.failed_configs.borrow().iter())
      .any(|key| key.split('\n').next().is_some_and(|source| Path::new(source) == file_path))
  }

  pub async fn file_info(self: &Rc<Self>, document: &DocumentInfo, text: Option<&str>, global_config: Option<GlobalConfigUse>) -> Result<FileInfoResult> {
    let document_file = match self.get_document_file(document) {
      Ok(document_file) => document_file,
      Err(err) => {
        return Ok(FileInfoResult {
          config_file: None,
          plugins: Vec::new(),
          status: FormatStatus::NotFormatted(into_unsupported_document(err)?),
        });
      }
    };
    let use_global_config = self.resolve_global_config_use(global_config);
    let is_notebook_cell = document_file.notebook_path.is_some();
    let file_path = document_file.candidates[0].clone();
    let target = ApprovalTarget::File {
      file_path: file_path.clone(),
      use_global_config,
    };
    let scope = match self.resolve_scope(&file_path, use_global_config).await? {
      DocumentConfig::Resolved(scope) => scope,
      DocumentConfig::NotFound => {
        let reason = self.get_no_config_reason(use_global_config);
        return Ok(FileInfoResult {
          config_file: None,
          plugins: Vec::new(),
          status: FormatStatus::NotFormatted(NotFormatted::new(
            reason,
            self.get_not_formatted_message(reason, is_notebook_cell, use_global_config),
            Vec::new(),
          )),
        });
      }
      DocumentConfig::Failed(failure) => {
        return Ok(FileInfoResult {
          plugins: Vec::new(),
          status: FormatStatus::NotFormatted(self.get_failure_not_formatted(&failure, &target)),
          config_file: failure.config_file,
        });
      }
    };
    // an extensionless file's shebang is read from the file on disk when the text isn't provided
    let file_bytes = text.map(|text| text.as_bytes());
    let chosen_file = self.choose_file(&scope, document_file, file_bytes);
    let reason = chosen_file.cant_format_reason;
    let plugins = match reason {
      Some(_) => Vec::new(),
      None => {
        let names = match file_bytes {
          Some(file_bytes) => scope
            .plugin_name_maps
            .get_plugin_names_from_file_path_and_bytes(&chosen_file.file_path, file_bytes)
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>(),
          None => get_plugin_names_for_file_on_disk(&scope.plugin_name_maps, &chosen_file.file_path, &self.environment)
            .iter()
            .map(|name| name.to_string())
            .collect(),
        };
        names
          .iter()
          .filter_map(|name| scope.plugins.get(name.as_str()))
          .map(|plugin| get_file_plugin(plugin.info()))
          .collect()
      }
    };
    let approvals = self.get_scope_approvals(&scope, &target);
    Ok(FileInfoResult {
      config_file: self.get_config_file_info(&scope),
      plugins,
      status: match reason {
        Some(reason) => FormatStatus::NotFormatted(self.new_scope_not_formatted(&scope, reason, is_notebook_cell, use_global_config, approvals)),
        None => FormatStatus::ready(),
      },
    })
  }

  pub async fn config_info(self: &Rc<Self>, target: &ConfigTarget, global_config: Option<GlobalConfigUse>) -> Result<ConfigInfoResult> {
    let use_global_config = self.resolve_global_config_use(global_config);
    let (target, config) = match self.resolve_config_target(target, use_global_config).await {
      Ok(config) => config,
      Err(err) => {
        return Ok(ConfigInfoResult {
          status: FormatStatus::NotFormatted(into_unsupported_document(err)?),
          ..Default::default()
        });
      }
    };
    let scope = match config {
      DocumentConfig::Resolved(scope) => scope,
      DocumentConfig::NotFound => {
        // a config file that's a dangling symlink is still watched by what it points to (see `config_files_changed`)
        let watch_uris = match &target {
          ApprovalTarget::ConfigFile { file_path, .. } => self.get_dangling_symlink_watch_uris(file_path),
          ApprovalTarget::File { .. } => Vec::new(),
        };
        return Ok(ConfigInfoResult {
          watch_uris,
          status: FormatStatus::NotFormatted(self.get_not_found_not_formatted(&target)),
          ..Default::default()
        });
      }
      DocumentConfig::Failed(failure) => {
        let config_key = failure.config_key.as_ref();
        let plugins = config_key.and_then(|(key, _)| self.last_plugin_infos.borrow().get(key).map(|last| last.plugins.clone()));
        let (file_patterns, other_files) = config_key
          .and_then(|(key, _)| self.last_file_patterns.borrow().get(key).cloned())
          .unwrap_or_default();
        // what it was resolved from is still watched so that fixing it is noticed
        let watch_uris = config_key
          .and_then(|(_, uri)| self.config_dependencies.borrow().get(uri.as_str()).cloned())
          .map(|dependencies| self.get_dependencies_watch_uris(&dependencies))
          .unwrap_or_default();
        return Ok(ConfigInfoResult {
          file_patterns,
          other_files,
          watch_uris,
          status: FormatStatus::NotFormatted(self.get_failure_not_formatted(&failure, &target)),
          plugins,
          config_file: failure.config_file,
        });
      }
    };
    let config = scope.config.as_ref();
    let reason = (!scope.untrusted_downloads.is_empty() || scope.unapproved_process_plugins.is_some()).then_some(NotFormattedReason::ApprovalRequired);
    let plugins = match config {
      // the plugins can't be used until they're approved, so this is what they formatted before
      Some(config) if reason.is_some() => self.get_last_plugin_infos(config),
      Some(config) => {
        let plugins = get_plugin_infos(&scope).await?;
        self.store_plugin_infos(config, &scope, &plugins).await;
        Some(plugins)
      }
      None => None,
    };
    let (file_patterns, other_files) = match (config, &plugins) {
      (Some(config), Some(plugins)) => {
        let patterns = get_file_patterns(config, plugins);
        self.last_file_patterns.borrow_mut().insert(get_config_key(config), patterns.clone());
        patterns
      }
      _ => Default::default(),
    };
    let watch_uris = config
      .map(|config| {
        let config_file = config.file_path().zip(config.source.maybe_local_path().map(|path| path.as_ref()));
        self.get_watch_uris(config_file, &config.local_dependencies)
      })
      .unwrap_or_default();
    let approvals = self.get_scope_approvals(&scope, &target);
    Ok(ConfigInfoResult {
      config_file: self.get_config_file_info(&scope),
      plugins,
      file_patterns,
      other_files,
      watch_uris,
      status: match reason {
        Some(reason) => FormatStatus::NotFormatted(self.new_scope_not_formatted(&scope, reason, false, use_global_config, approvals)),
        None => FormatStatus::ready(),
      },
    })
  }

  /// Gets the plugins of the config file from what they matched the last time
  /// it was resolved, which is `None` when that isn't known for each of them.
  fn get_last_plugin_infos(&self, config: &ResolvedConfig) -> Option<Vec<PluginInfo>> {
    let config_key = get_config_key(config);
    // they're of other plugins once a plugin was added or changed
    if let Some(last) = self.last_plugin_infos.borrow().get(&config_key)
      && last.plugin_references == get_plugin_references(config)
    {
      return Some(last.plugins.clone());
    }
    // a plugin's information is in the plugin cache, which is kept across
    // sessions, and what it matched is the config file's because that may
    // depend on the plugin's configuration (ex. the exec plugin's commands)
    config
      .plugins
      .iter()
      .map(|plugin| {
        let info = self.scope_container.get_set_up_plugin_info(plugin)?;
        let file_matching = self.scope_container.get_last_file_matching_info(plugin, &config_key)?;
        Some(get_plugin_info(info, file_matching))
      })
      .collect()
  }

  /// Remembers the plugins of the config file for when they can't be resolved
  /// (ex. the config file has an error or the user needs to approve them).
  async fn store_plugin_infos(&self, config: &Rc<ResolvedConfig>, scope: &PluginsScope<TEnvironment>, plugins: &[PluginInfo]) {
    let config_key = get_config_key(config);
    self.last_plugin_infos.borrow_mut().insert(
      config_key.clone(),
      LastPluginInfos {
        plugin_references: get_plugin_references(config),
        plugins: plugins.to_vec(),
        config: config.clone(),
      },
    );
    for plugin in config.plugins.iter() {
      let Some(plugin_with_config) = self
        .scope_container
        .get_set_up_plugin_info(plugin)
        .and_then(|info| scope.plugins.values().find(|resolved| resolved.info().name == info.name))
      else {
        continue;
      };
      if let Err(err) = self
        .scope_container
        .store_file_matching_info(plugin, &config_key, &plugin_with_config.file_matching)
        .await
      {
        log_debug!(self.environment, "Failed storing the files {} matches: {:#}", plugin.display(), err);
      }
    }
  }

  /// Gets the schema of the config file, which says whether it has the
  /// schema of each plugin. The editor extension is told the config file
  /// changed once the schemas that didn't download in time did. It fails like
  /// `config_info` for params that can't be handled (ex. a file that isn't a
  /// config file).
  pub async fn config_schema(self: &Rc<Self>, target: &ConfigTarget, global_config: Option<GlobalConfigUse>) -> Result<(serde_json::Value, bool)> {
    let mut plugins = Vec::new();
    let use_global_config = self.resolve_global_config_use(global_config);
    // a config file that fails to resolve was shown to the user, so its plugins are only left out
    let mut is_resolved = false;
    let mut config_changed = None;
    let resolved = match self.resolve_config_target(target, use_global_config).await {
      Ok((_, config)) => Some(config),
      Err(err) if err.downcast_ref::<InvalidParamsError>().is_some() => return Err(err),
      Err(_) => None,
    };
    if let Some(DocumentConfig::Resolved(scope)) = resolved {
      config_changed = scope.config.as_ref().and_then(|config| self.new_config_changed(config, ConfigChange::Changed));
      // the plugins aren't known while they need to be approved
      is_resolved = scope.untrusted_downloads.is_empty() && scope.unapproved_process_plugins.is_none();
      plugins.extend(scope.plugins.values().map(|plugin| PluginSchemaUrl {
        config_key: plugin.info().config_key.clone(),
        url: plugin.info().config_schema_url.clone(),
      }));
    }
    let schema = self.config_schema_builder.build(&plugins).await;
    // only wait on the downloads once for the config file
    if schema.is_downloading
      && let Some(config_changed) = config_changed
      && let Some(wait_key) = get_config_changed_key(&config_changed)
      && self.schema_waits.borrow_mut().insert(wait_key.clone())
    {
      let workspace = self.clone();
      dprint_core::async_runtime::spawn(async move {
        let urls = plugins.iter().map(|plugin| plugin.url.as_str()).collect::<Vec<_>>();
        workspace.config_schema_builder.wait_for_downloads(&urls).await;
        workspace.schema_waits.borrow_mut().remove(&wait_key);
        // a schema that failed to download doesn't change it
        if workspace.config_schema_builder.is_any_downloaded(&urls) {
          workspace.send_config_changed(ConfigChangedParams {
            schema_only: true,
            ..config_changed
          });
        }
      });
    }
    Ok((schema.schema, is_resolved && schema.is_complete))
  }

  /// Gets what needs to be approved for the config files that were used,
  /// which resolves them again in case something changed (ex. the user
  /// allowed a process plugin by running `dprint fmt`).
  pub async fn pending_approvals(
    self: &Rc<Self>,
    config_file: Option<(Uri, Option<Uri>)>,
  ) -> Result<(Vec<PendingApproval>, Option<(Option<ConfigFileInfo>, FormatStatus)>)> {
    // what's pending for one config file says whether it's used
    if let Some((uri, dir_uri)) = config_file {
      let (target, config) = match self.resolve_config_target(&ConfigTarget::ConfigFile { uri, dir_uri }, false).await {
        Ok(result) => result,
        Err(err) => {
          let not_formatted = into_unsupported_document(err)?;
          return Ok((Vec::new(), Some((None, FormatStatus::NotFormatted(not_formatted)))));
        }
      };
      let (config_file, approvals, status) = self.get_approvals_and_status(config, &target);
      return Ok((approvals, Some((config_file, status))));
    }
    let targets = self
      .approvals
      .borrow()
      .values()
      .filter_map(|stored| stored.first())
      .map(|stored| stored.config_target.clone())
      .collect::<Vec<_>>();
    let mut approvals = Vec::new();
    for target in targets {
      approvals.extend(self.get_approvals_for_target(&target).await);
    }
    Ok((approvals, None))
  }

  /// Resolves the config file of the target and gets what needs to be approved for it.
  async fn get_approvals_for_target(self: &Rc<Self>, target: &ApprovalTarget) -> Vec<PendingApproval> {
    match self.resolve_approval_target(target).await {
      Ok(DocumentConfig::Resolved(scope)) => self.get_scope_approvals(&scope, target),
      Ok(DocumentConfig::Failed(failure)) => self.get_failure_approvals(&failure, target),
      Ok(DocumentConfig::NotFound) => {
        self.forget_missing_config_approvals(target);
        Vec::new()
      }
      Err(_) => Vec::new(),
    }
  }

  /// Resolves the config file of something that needs to be approved.
  async fn resolve_approval_target(self: &Rc<Self>, target: &ApprovalTarget) -> Result<DocumentConfig<TEnvironment>> {
    match target {
      ApprovalTarget::File { file_path, use_global_config } => self.resolve_scope(file_path, *use_global_config).await,
      ApprovalTarget::ConfigFile { file_path, dir_path } => match self.load_config_file(file_path, dir_path.as_deref()) {
        Ok(Some(config_file)) => self.resolve_found_config(config_file).await,
        Ok(None) => Ok(DocumentConfig::NotFound),
        Err(err) => match find_config_file_on_another_machine(&err) {
          Some(err) => Ok(DocumentConfig::Failed(self.new_config_file_on_another_machine_failure(err))),
          None => Err(err),
        },
      },
    }
  }

  /// Resolves the config file of a config target.
  async fn resolve_config_target(self: &Rc<Self>, target: &ConfigTarget, use_global_config: bool) -> Result<(ApprovalTarget, DocumentConfig<TEnvironment>)> {
    match target {
      ConfigTarget::ConfigFile { uri, dir_uri } => {
        // the server uses one config file for every file (`dprint lsp --config <path>`)
        if self.scope_container.has_config_override() {
          let dir_path = match dir_uri.as_ref().and_then(uri_to_file_path) {
            Some(dir_path) => self.canonicalize_editor_path(dir_path),
            None => self.environment.cwd().into_path_buf(),
          };
          if !self.is_config_override_uri(uri) {
            return Err(InvalidParamsError(format!("The configFile isn't the config file the server was started with: {}", uri.as_str())).into());
          }
          let target = ApprovalTarget::File {
            file_path: dir_path.join("file"),
            use_global_config,
          };
          let config = self.resolve_approval_target(&target).await?;
          return Ok((target, config));
        }
        let Some(config_file_path) = uri_to_file_path(uri).filter(|path| is_config_file_path(&self.environment, path)) else {
          return Err(InvalidParamsError(format!("The configFile isn't a config file: {}", uri.as_str())).into());
        };
        let dir_path = dir_uri.as_ref().and_then(uri_to_file_path);
        let config_file = match self.load_config_file(&config_file_path, dir_path.as_deref()) {
          Ok(Some(config_file)) => config_file,
          // it was deleted
          Ok(None) => {
            return Ok((
              ApprovalTarget::ConfigFile {
                file_path: config_file_path,
                dir_path,
              },
              DocumentConfig::NotFound,
            ));
          }
          Err(err) => {
            // it's read once the user allows reading it from the other machine
            let failure = match find_config_file_on_another_machine(&err) {
              Some(err) => self.new_config_file_on_another_machine_failure(err),
              None => ConfigFailure {
                config_file: None,
                config_key: None,
                config_target: None,
                reason: NotFormattedReason::ConfigError,
                message: format!("{:#}", err),
                downloads: Vec::new(),
              },
            };
            return Ok((
              ApprovalTarget::ConfigFile {
                file_path: config_file_path,
                dir_path,
              },
              DocumentConfig::Failed(failure),
            ));
          }
        };
        // the global config file is resolved for the directory it was loaded for
        let dir_path = config_file.is_global_config.then(|| config_file.base_path.as_ref().to_path_buf());
        let target = ApprovalTarget::ConfigFile {
          file_path: config_file_path,
          dir_path,
        };
        let config = self.resolve_found_config(config_file).await?;
        Ok((target, config))
      }
      ConfigTarget::Document(document) => {
        let file_path = self.get_document_file(document)?.candidates.remove(0);
        let config = self.resolve_scope(&file_path, use_global_config).await?;
        let target = ApprovalTarget::File { file_path, use_global_config };
        Ok((target, config))
      }
    }
  }

  /// Gets whether the uri is of the config file the server was started with,
  /// which may not exist right now. It's compared by the path that identifies
  /// it (see `canonicalize_config_file_path`), so another symlink to the same
  /// file is another config file.
  fn is_config_override_uri(&self, uri: &Uri) -> bool {
    match (uri_to_file_path(uri), self.scope_container.config_override_path()) {
      // the one the server was started with is the user's, but the path is from the editor
      (Some(path), Some(override_path)) => self.canonicalize_config_file_path(path) == self.canonicalize_config_file_path(override_path.to_path_buf()),
      _ => false,
    }
  }

  /// Gets whether a file the editor said changed is the config file the
  /// server was started with or what it points to (a change of either is
  /// one of it), which are compared by their canonical paths.
  fn is_config_override_file(&self, path: &Path) -> bool {
    match self.scope_container.config_override_path() {
      // the one the server was started with is the user's, but the path is from the editor
      Some(override_path) => self.canonicalize_editor_path(path.to_path_buf()) == self.canonicalize(override_path),
      None => false,
    }
  }

  /// Formats the document, which says the config file that was used.
  pub async fn format(self: &Rc<Self>, request: FormatRequest, token: &Arc<CancellationToken>) -> Result<(FormatOutcome, Option<ConfigFileInfo>)> {
    let document_file = match self.get_document_file(&request.document) {
      Ok(document_file) => document_file,
      Err(err) => return Ok((FormatOutcome::NotFormatted(into_unsupported_document(err)?), None)),
    };
    let use_global_config = self.resolve_global_config_use(request.global_config);
    let is_notebook_cell = document_file.notebook_path.is_some();
    let file_path = document_file.candidates[0].clone();
    let target = ApprovalTarget::File {
      file_path: file_path.clone(),
      use_global_config,
    };
    let scope = match self.resolve_scope(&file_path, use_global_config).await? {
      DocumentConfig::Resolved(scope) => scope,
      DocumentConfig::NotFound => {
        log_stderr_info!(self.environment, "Path did not have a dprint config file: {}", file_path.display());
        let reason = self.get_no_config_reason(use_global_config);
        if request.notify_no_config {
          self.notify_no_config(reason, use_global_config);
        }
        let not_formatted = NotFormatted::new(reason, self.get_not_formatted_message(reason, is_notebook_cell, use_global_config), Vec::new());
        return Ok((FormatOutcome::NotFormatted(not_formatted), None));
      }
      DocumentConfig::Failed(failure) => {
        let not_formatted = self.get_failure_not_formatted(&failure, &target);
        self.prompt_for_approvals(&not_formatted.approvals);
        return Ok((FormatOutcome::NotFormatted(not_formatted), failure.config_file));
      }
    };
    let outcome = self
      .format_with_scope(scope.clone(), request, document_file, target, use_global_config, token)
      .await?;
    // it's after formatting, which is when it's provided
    let config_file = self.get_config_file_info(&scope);
    Ok((outcome, config_file))
  }

  /// Formats the document with the plugins of its config file.
  async fn format_with_scope(
    self: &Rc<Self>,
    scope: Rc<PluginsScope<TEnvironment>>,
    request: FormatRequest,
    document_file: DocumentFile,
    target: ApprovalTarget,
    use_global_config: bool,
    token: &Arc<CancellationToken>,
  ) -> Result<FormatOutcome> {
    let is_notebook_cell = document_file.notebook_path.is_some();
    let approvals = self.get_scope_approvals(&scope, &target);
    let chosen_file = self.choose_file(&scope, document_file, Some(request.text.as_bytes()));
    // only asked when formatting a file the config file formats because the other
    // requests are made without the user doing anything (ex. when a project is opened)
    if chosen_file.cant_format_reason == Some(NotFormattedReason::ApprovalRequired) {
      self.prompt_for_approvals(&approvals);
    }
    let not_formatted = |reason| {
      Ok(FormatOutcome::NotFormatted(self.new_scope_not_formatted(
        &scope,
        reason,
        is_notebook_cell,
        use_global_config,
        approvals.clone(),
      )))
    };
    if let Some(reason) = chosen_file.cant_format_reason {
      return not_formatted(reason);
    }
    let file_path = chosen_file.file_path;
    let file_text = request.text;
    // `None` formats the whole text
    let mut ranges = match &request.ranges {
      Some(ranges) => get_utf16_ranges(&file_text, ranges),
      None => vec![None],
    };
    if ranges.is_empty() {
      return Ok(FormatOutcome::Formatted(Vec::new()));
    }
    let plugins_format_range = match ranges.iter().any(|range| range.is_some()) {
      true => match scope.range_formatting_support(&file_path, file_text.as_bytes()) {
        RangeFormattingSupport::Supported => true,
        RangeFormattingSupport::Unknown => false,
        RangeFormattingSupport::NotSupported => return not_formatted(NotFormattedReason::RangeFormattingNotSupported),
      },
      false => true,
    };

    let _permit = self.concurrency_limiter.acquire().await;
    let ensure_stable_format = self
      .settings
      .borrow()
      .ensure_stable_format
      .map(EnsureStableFormat)
      .unwrap_or(self.ensure_stable_format);
    let start_time = std::time::Instant::now();
    // each range is formatted in the text that formatting the ones before it changed
    let file_text = Arc::new(file_text);
    let mut text = file_text.clone();
    let mut edits = Vec::new();
    for index in 0..ranges.len() {
      if token.is_cancelled() {
        return Ok(FormatOutcome::Formatted(Vec::new()));
      }
      let range = ranges[index].as_ref().and_then(|range| get_byte_range(&text, range.start, range.end));
      let range_format = RangeFormat::new(&text, range, plugins_format_range);
      let result = scope
        .format_stable(
          HostFormatRequest {
            file_path: file_path.clone(),
            file_bytes: text.as_bytes().to_vec(),
            range: range_format.plugin_range.clone(),
            override_config: request.override_config.clone(),
            token: token.clone(),
          },
          ensure_stable_format,
        )
        .await;
      let formatted_text = match result {
        Ok(Some(formatted_text)) => formatted_text,
        Ok(None) => continue,
        Err(err) => {
          if let Some(err) = err.downcast_ref::<PluginConfigDiagnosticsError>() {
            return Ok(self.get_plugin_config_diagnostics_outcome(&scope, &err.diagnostics));
          }
          let reason = if err.downcast_ref::<CriticalFormatError>().is_some() {
            FormatFailedReason::PluginError
          } else {
            FormatFailedReason::FormatError
          };
          // the plugin that failed is only known when one plugin formats the file
          let plugin_names = scope.plugin_name_maps.get_plugin_names_from_file_path_and_bytes(&file_path, text.as_bytes());
          let plugin = match plugin_names.iter().collect::<Vec<_>>().as_slice() {
            [name] => scope.plugins.get(*name).map(|plugin| get_file_plugin(plugin.info())),
            _ => None,
          };
          return Ok(FormatOutcome::Failed {
            reason,
            message: dprint_core::plugins::error_to_string(&err),
            plugin,
          });
        }
      };
      // this may take a while for a large file, so don't hold up the other requests
      let range_edits = dprint_core::async_runtime::spawn_blocking({
        let text = text.clone();
        move || {
          let formatted_text = String::from_utf8(formatted_text).map_err(|_| anyhow::anyhow!("The formatted text isn't valid UTF-8."))?;
          // the editor owns the document's line endings, so match them rather than
          // imposing the plugin's configured newline kind (see #965)
          let formatted_text = normalize_to_source_line_endings(&text, formatted_text);
          let formatted_text = if is_notebook_cell {
            trim_formatted_cell_text(&text, formatted_text, &range_format.cell_range())
          } else {
            formatted_text
          };
          Ok::<_, anyhow::Error>(get_range_utf16_edits(&text, &formatted_text, &range_format.edits_range))
        }
      })
      .await??;
      if range_edits.is_empty() {
        continue;
      }
      if ranges.len() == 1 {
        edits = range_edits;
        break;
      }
      for later_range in ranges[index + 1..].iter_mut().flatten() {
        *later_range = map_utf16_range(later_range.clone(), &range_edits);
      }
      text = Arc::new(apply_utf16_edits(&text, &range_edits));
    }
    log_debug!(
      self.environment,
      "Formatted {} in {}ms",
      request.document.uri.as_str(),
      start_time.elapsed().as_millis()
    );
    // the edits of each range are for the text that the ones before it changed,
    // so get the edits of the text that was sent instead
    if ranges.len() > 1 && !Arc::ptr_eq(&text, &file_text) {
      edits = dprint_core::async_runtime::spawn_blocking(move || get_utf16_edits(&file_text, &text)).await?;
    }
    Ok(FormatOutcome::Formatted(edits))
  }

  /// Approves what was pending for a config file, which resolves to what needs
  /// to be approved now. That has what was denied when it was denied and the
  /// denial was kept (see `ALLOWED_SINCE_MESSAGE`), and otherwise what else
  /// needs to be approved (ex. a config file that a downloaded one extends),
  /// which is empty once nothing does. Approving a
  /// process plugin that isn't set up allows setting it up, which sets it up.
  ///
  /// The decision is used once this is called, even when the request is
  /// cancelled while what was allowed is set up.
  ///
  /// It's with what to tell the user about the decision (ex. it couldn't be
  /// stored), which may be about a decision made before (ex. an "Always
  /// Allow" for process plugins that weren't set up is stored once this sets
  /// them up).
  pub async fn approve(self: &Rc<Self>, id: &str, decision: ApprovalDecision) -> Result<ApproveResult> {
    let stored = self.approvals.borrow().values().flatten().find(|stored| stored.approval.id == id).cloned();
    let Some(stored) = stored else {
      // a denial of downloads that were each allowed exactly since (ex. for another config file, which resolved
      // this one again without the approval) wouldn't do anything, which the user is told like below, unless
      // the config file can't be resolved right now
      let provided = self.approval_files.borrow().get(id).cloned();
      if let Some(ProvidedApproval {
        target,
        downloads: Some(downloads),
      }) = provided
        && !decision.is_allowed()
        && self.is_each_allowed(&downloads)
        // it's still an approval that changed when the config file can't be resolved right now
        && let Ok(config) = self.resolve_approval_target(&target).await
      {
        let (config_file, approvals, status) = self.get_approvals_and_status(config, &target);
        return Ok(ApproveResult {
          config_approvals: ConfigApprovals {
            config_file,
            approvals,
            status,
          },
          notice: Some(new_warning_notice(ALLOWED_SINCE_MESSAGE.to_string())),
        });
      }
      return Err(self.new_approval_changed_error(id).await);
    };
    if !stored.approval.decisions.iter().any(|option| option.decision == decision) {
      return Err(InvalidParamsError(format!("The decision {:?} isn't one of the approval's decisions.", decision)).into());
    }
    // failing to store a decision for process plugins once they're set up is only told about
    // when what this allows sets them up (ex. not when it was a while ago)
    let store_error_mark = self.session_permissions.process_plugin_store_error_mark();
    let mut notice = None;
    match &stored.subject {
      ApprovalSubject::Downloads(urls) => {
        if decision.is_allowed() {
          self.environment.download_policy().allow(urls);
          // the items are of the downloads in the same order
          let mut allowed_items = self.allowed_download_items.borrow_mut();
          for (url, item) in urls.iter().zip(&stored.approval.items) {
            allowed_items.insert(url.clone(), item.clone());
          }
          // a denial of what's allowed now (ex. with the approve command) is replaced, because an allowed
          // download is allowed for every config file
          let allowed = self.environment.download_policy().allowed_downloads().into_iter().collect::<HashSet<_>>();
          self
            .denied_downloads
            .borrow_mut()
            .retain(|_, denied| !denied.downloads.iter().all(|download| allowed.contains(download)));
          drop(allowed_items);
          self.denied_downloads_changed();
        } else if self.is_each_allowed(urls) {
          // a denial of downloads that are each allowed exactly since (ex. for another config file) wouldn't do
          // anything, because an exact allow wins over a denial, so it isn't kept
          notice = Some(new_warning_notice(ALLOWED_SINCE_MESSAGE.to_string()));
        } else {
          self.denied_downloads.borrow_mut().insert(
            stored.approval.id.clone(),
            DeniedDownload {
              id: stored.approval.id.clone(),
              config_key: stored.config_key.clone(),
              downloads: urls.clone(),
              config_file: ConfigFileIdentifier {
                uri: stored.approval.config_file.uri.clone(),
                dir_uri: Some(stored.approval.config_file.dir_uri.clone()),
              },
              items: stored.approval.items.clone(),
            },
          );
          self.denied_downloads_changed();
        }
      }
      ApprovalSubject::ProcessPlugins(unapproved) => {
        let title = get_process_plugin_action_title(decision);
        if let Err(err) = store_process_plugin_action(&self.environment, &self.session_permissions, unapproved, Some(title)).await {
          let message = get_process_plugin_store_error_message(&err);
          log_warn!(self.environment, "{}", message);
          notice = Some(new_warning_notice(message));
        }
      }
    }
    // this downloads and sets up what was allowed, and tells the editor that
    // the config file formats files now, which is the config file of the
    // approval even when a document it was for uses another one now
    let target = &stored.config_target;
    let config = self.resolve_approval_target(target).await;
    // what the user always allowed for a process plugin that wasn't set up is stored once it's set up,
    // which is usually while resolving it here (ex. after allowing it or what it needed downloaded)
    if let Ok(DocumentConfig::Resolved(scope)) = &config
      && let Some(config) = &scope.config
      && let Some(message) = self.session_permissions.take_process_plugin_store_error(&config.display(), store_error_mark)
    {
      push_warning_notice(&mut notice, message);
    }
    let is_download_pending = match &config {
      Ok(DocumentConfig::Resolved(scope)) => !scope.untrusted_downloads.is_empty(),
      Ok(DocumentConfig::Failed(failure)) => !failure.downloads.is_empty(),
      _ => false,
    };
    if let ApprovalSubject::ProcessPlugins(unapproved) = &stored.subject
      && !is_download_pending
    {
      // allowing a plugin to be set up is only for this, so a plugin that
      // wasn't set up (ex. it failed to) needs to be approved again, but not
      // when what needs to be downloaded first wasn't allowed yet
      for plugin in &unapproved.plugins {
        self
          .session_permissions
          .take_process_plugin_set_up_grant(&unapproved.config_file, &plugin.plugin);
      }
    }
    // the other config files that were waiting on what was allowed to be downloaded can be used now too,
    // which the editor is told about
    if decision.is_allowed() && matches!(stored.subject, ApprovalSubject::Downloads(_)) {
      let workspace = self.clone();
      dprint_core::async_runtime::spawn(async move { workspace.resolve_used_configs_again().await });
    }
    // the decision is kept before responding so a server started again for the session uses it
    if let Err(err) = self.save_session() {
      let message = err.to_string();
      log_warn!(self.environment, "{}", message);
      push_warning_notice(&mut notice, message);
    }
    // the editor is told once reading a config file on another machine is allowed, so it gets the results again
    if decision.is_allowed() && stored.approval.config_file.on_another_machine {
      self.send_config_changed(ConfigChangedParams {
        config_file: Some(ConfigFileIdentifier {
          uri: stored.approval.config_file.uri.clone(),
          dir_uri: Some(stored.approval.config_file.dir_uri.clone()),
        }),
        change: Some(ConfigChange::Changed),
        schema_only: false,
      });
    }
    let config = config?;
    // a config file that still fails didn't resolve to something else, so the editor is told it was denied
    if let DocumentConfig::Failed(failure) = &config
      && !decision.is_allowed()
      && let Some(config_file) = &failure.config_file
    {
      self.send_config_changed(ConfigChangedParams {
        config_file: Some(ConfigFileIdentifier {
          uri: config_file.uri.clone(),
          dir_uri: Some(config_file.dir_uri.clone()),
        }),
        change: Some(ConfigChange::Changed),
        schema_only: false,
      });
    }
    let (config_file, approvals, status) = self.get_approvals_and_status(config, target);
    Ok(ApproveResult {
      config_approvals: ConfigApprovals {
        config_file,
        approvals,
        status,
      },
      notice,
    })
  }

  /// Gets the error for approving what isn't pending anymore, which says what
  /// needs to be approved now when the config file is known.
  async fn new_approval_changed_error(self: &Rc<Self>, id: &str) -> anyhow::Error {
    let target = self.approval_files.borrow().get(id).map(|provided| provided.target.clone());
    let now = match target {
      // it's still an approval that changed when the config file can't be resolved right now
      Some(target) => match self.resolve_approval_target(&target).await {
        Ok(config) => Some(self.get_approvals_and_status(config, &target)),
        Err(_) => None,
      },
      None => None,
    };
    ApprovalChangedError {
      id: id.to_string(),
      current: now.map(|(config_file, approvals, status)| ConfigApprovals {
        config_file,
        approvals,
        status,
      }),
    }
    .into()
  }

  /// Gets the config file of the target, what needs to be approved for it,
  /// and whether it's used.
  fn get_approvals_and_status(
    &self,
    config: DocumentConfig<TEnvironment>,
    target: &ApprovalTarget,
  ) -> (Option<ConfigFileInfo>, Vec<PendingApproval>, FormatStatus) {
    match config {
      DocumentConfig::Resolved(scope) => {
        let approvals = self.get_scope_approvals(&scope, target);
        let status = match approvals.is_empty() {
          true => FormatStatus::ready(),
          false => FormatStatus::NotFormatted(self.new_scope_not_formatted(&scope, NotFormattedReason::ApprovalRequired, false, false, approvals.clone())),
        };
        (self.get_config_file_info(&scope), approvals, status)
      }
      DocumentConfig::Failed(failure) => {
        let not_formatted = self.get_failure_not_formatted(&failure, target);
        (failure.config_file, not_formatted.approvals.clone(), FormatStatus::NotFormatted(not_formatted))
      }
      DocumentConfig::NotFound => {
        self.forget_missing_config_approvals(target);
        (None, Vec::new(), FormatStatus::NotFormatted(self.get_not_found_not_formatted(target)))
      }
    }
  }

  /// Gets what a config file is resolved for by the path that identifies it,
  /// which is `None` when it's resolved for a file instead (ex. it's remote or
  /// the one the server was started with).
  fn get_config_file_target(&self, config_file: ConfigFileRef<'_>) -> Option<ApprovalTarget> {
    if self.scope_container.has_config_override() {
      return None;
    }
    Some(ApprovalTarget::ConfigFile {
      file_path: config_file.file_path?.to_path_buf(),
      dir_path: config_file.is_global.then(|| config_file.base_path.to_path_buf()),
    })
  }

  /// Forgets what needs to be approved for the config file of the target when
  /// it doesn't exist anymore, which isn't approvable then.
  fn forget_missing_config_approvals(&self, target: &ApprovalTarget) {
    if let ApprovalTarget::ConfigFile { file_path, .. } = target
      && let Some(uri) = file_path_to_uri(&self.canonicalize_config_file_path(file_path.clone()))
    {
      self.forget_config_file_approvals(&uri);
    }
  }

  /// Forgets what needs to be approved for the config file, which is for each
  /// directory the global config file is used for.
  fn forget_config_file_approvals(&self, uri: &Uri) {
    let directory_prefix = format!("{}\n", uri.as_str());
    self
      .approvals
      .borrow_mut()
      .retain(|key, _| key != uri.as_str() && !key.starts_with(&directory_prefix));
  }

  /// Forgets what the user decided for the config file, or for every config
  /// file when `None`, which they're asked about again (ex. to undo allowing
  /// a process plugin). That includes the "Always" decisions.
  ///
  /// It's with what to tell the user about forgetting them (ex. the stored
  /// decisions couldn't be forgotten).
  pub async fn forget_decisions(self: &Rc<Self>, config_file: Option<(Uri, Option<Uri>)>) -> Result<(Vec<Decision>, Option<Notice>)> {
    // the config file as the process plugin decisions have it and its key, which is found without
    // resolving it because that would run what the user wants to stop allowing
    let identifier = config_file;
    let config_file = match &identifier {
      Some((uri, dir_uri)) => Some(self.get_decisions_config_file(uri, dir_uri.as_ref())?),
      None => None,
    };
    // an "Always" decision that's still being stored (ex. while another process holds the approvals
    // file's lock) is forgotten too, which the stores wait on until the stored decisions are forgotten
    let paused_stores = self.session_permissions.pause_stores().await;
    // the downloads allowed for the session stay allowed (ex. they're in the cache already)
    let forgotten = self
      .get_decisions(config_file.as_ref(), identifier.as_ref())
      .into_iter()
      .filter(|decision| decision.config_file.is_some())
      .collect::<Vec<_>>();
    let is_config_file = |source: &str| self.is_decisions_config_file(source, config_file.as_ref(), identifier.as_ref());
    self.session_permissions.forget_process_plugin_decisions(&is_config_file);
    self
      .denied_downloads
      .borrow_mut()
      .retain(|_, denied| !is_denial_for(denied, config_file.as_ref(), identifier.as_ref()));
    self.denied_downloads_changed();
    let session_result = self.save_session();
    let stored_result = self.forget_stored_decisions(&is_config_file).await;
    // resolving them may store what the user always allowed once it's set up
    drop(paused_stores);
    self.resolve_used_configs_again().await;
    // what was forgotten for the session still is when the stored decisions can't be
    let mut notice = stored_result.err().map(|err| {
      let message = match err.chain().find_map(|err| err.downcast_ref::<NewerApprovalsFileError>()) {
        // this version never writes to the file (see `EditorApprovals`)
        Some(err) => format!(
          "This version of dprint is older than the one that stored the approval decisions in {}, so the ones stored there couldn't be forgotten. Update dprint to forget them.",
          err.file_path.display()
        ),
        None => format!("Failed forgetting the decisions that were stored: {:#}", err),
      };
      log_warn!(self.environment, "{}", message);
      new_warning_notice(message)
    });
    // the session file may still have the decisions that were forgotten
    if let Err(err) = session_result {
      let message = err.to_string();
      log_warn!(self.environment, "{}", message);
      push_warning_notice(&mut notice, message);
    }
    Ok((forgotten, notice))
  }

  /// Gets what the user decided for the config file, or for every config file
  /// when `None`, which says whether the settings file allows every process
  /// plugin.
  pub fn decisions(&self, config_file: Option<(Uri, Option<Uri>)>) -> Result<(Vec<Decision>, bool)> {
    let identifier = config_file;
    let config_file = match &identifier {
      Some((uri, dir_uri)) => Some(self.get_decisions_config_file(uri, dir_uri.as_ref())?),
      None => None,
    };
    let allow_process_plugins = self.session_permissions.read(&self.environment).allow_process_plugins == Some(true);
    Ok((self.get_decisions(config_file.as_ref(), identifier.as_ref()), allow_process_plugins))
  }

  /// Gets what the user decided for the config file as the process plugin
  /// decisions have it with its key, or for every config file when `None`.
  /// The identifier is what the editor sent for it (see `is_denial_for`).
  fn get_decisions(&self, config_file: Option<&(String, String)>, identifier: Option<&(Uri, Option<Uri>)>) -> Vec<Decision> {
    let is_config_file = |source: &str| self.is_decisions_config_file(source, config_file, identifier);
    // the stored decisions are read even when the settings allow every process plugin
    // the denials of a file that can't be read are still used, so they're provided too, and
    // only they're used when the settings aren't (see `SessionPermissions::read`)
    let stored = match read_ignored_settings_message(&self.environment) {
      Some(_) => read_editor_approval_denials(&self.environment),
      None => read_editor_approvals(&self.environment)
        .map(|approvals| approvals.process_plugin_decisions)
        .unwrap_or_else(|_| read_editor_approval_denials(&self.environment)),
    };
    let session = self.session_permissions.session_process_plugin_decisions();
    // allowing a plugin that isn't set up is a grant until it's set up
    let grants = self.session_permissions.process_plugin_set_up_grants();
    let candidates = stored
      .iter()
      // an allow that this version doesn't use isn't what it decides with (ex. its config hash is made another way)
      .filter(|decision| decision.is_used())
      .map(|decision| {
        (
          &decision.approval.config_file,
          &decision.approval.plugin,
          decision.allowed,
          true,
          decision.decided_at,
        )
      })
      .chain(session.iter().map(|decision| {
        (
          &decision.approval.config_file,
          &decision.approval.plugin,
          decision.allowed,
          false,
          decision.decided_at,
        )
      }))
      .chain(
        grants
          .iter()
          .map(|grant| (&grant.config_file, &grant.plugin, true, grant.always, grant.decided_at)),
      );
    // the most recent decision for a plugin of a config file is what's used, which is a denial
    // on a tie and otherwise the session's or a grant like when formatting
    let mut plugin_decisions: Vec<(String, String, bool, bool, u64)> = Vec::new();
    for (config_file, plugin, allows, always, decided_at) in candidates {
      if !is_config_file(config_file) {
        continue;
      }
      let existing = plugin_decisions.iter().position(|(c, p, ..)| c == config_file && p == plugin);
      if let Some(index) = existing {
        let (_, _, existing_allows, _, existing_decided_at) = &plugin_decisions[index];
        if is_more_recent_at(*existing_decided_at, *existing_allows, decided_at, allows) {
          continue;
        }
        plugin_decisions.remove(index);
      }
      plugin_decisions.push((config_file.clone(), plugin.clone(), allows, always, decided_at));
    }
    let global_config_path = self.get_global_config_path();
    let mut decisions = Vec::new();
    for (config_file, plugin, allows, always, _) in plugin_decisions {
      decisions.push(Decision {
        kind: ApprovalKind::ProcessPlugins,
        config_file: self.get_listed_config_file(&config_file, global_config_path.as_deref()),
        // like the specifier of its item
        specifiers: vec![escape_text(&plugin)],
        items: None,
        allows,
        always,
      });
    }
    for denied in self.denied_downloads.borrow().values() {
      if is_denial_for(denied, config_file, identifier) {
        // a restored denial may not have every item (ex. one of a kind a newer version added),
        // which then shows each download it's for, with the items that can be told from them
        let has_every_item = denied.items.len() == denied.downloads.len();
        decisions.push(Decision {
          kind: ApprovalKind::Downloads,
          config_file: Some(self.get_denial_config_file(denied)),
          specifiers: match has_every_item {
            true => denied.items.iter().map(|item| item.specifier.clone()).collect(),
            false => denied.downloads.iter().map(|download| get_download_specifier(download)).collect(),
          },
          items: match has_every_item {
            true => Some(
              denied
                .items
                .iter()
                .zip(&denied.downloads)
                .map(|(item, download)| self.with_current_editor_uris(item.clone(), download))
                .collect(),
            ),
            false => denied
              .downloads
              .iter()
              .map(|download| {
                let specifier = get_download_specifier(download);
                denied
                  .items
                  .iter()
                  .find(|item| item.specifier == specifier)
                  .map(|item| self.with_current_editor_uris(item.clone(), download))
                  .or_else(|| self.get_download_item_from_text(download))
              })
              .collect(),
          },
          allows: false,
          always: false,
        });
      }
    }
    // what was allowed to be downloaded is allowed for every config file
    let mut allowed_downloads = self.environment.download_policy().allowed_downloads();
    allowed_downloads.sort();
    let allowed_items = self.allowed_download_items.borrow();
    decisions.extend(allowed_downloads.into_iter().map(|download| {
      let item = allowed_items.get(&download);
      Decision {
        kind: ApprovalKind::Downloads,
        config_file: None,
        specifiers: vec![item.map(|item| item.specifier.clone()).unwrap_or_else(|| get_download_specifier(&download))],
        items: item
          .map(|item| self.with_current_editor_uris(item.clone(), &download))
          .or_else(|| self.get_download_item_from_text(&download))
          .map(|item| vec![item]),
        allows: true,
        always: false,
      }
    }));
    decisions
  }

  /// Gets whether a process plugin decision stored with the config file's text
  /// is for the config file as the process plugin decisions have it with its
  /// key, or for every config file when `None`.
  ///
  /// It's also for the config file the editor sent when that's how
  /// `dprint/decisions` lists it, so what's listed can always be forgotten,
  /// even when the config file's text isn't the one this version makes for it
  /// (ex. the project was moved since and its old path is a symlink to it, or
  /// another version made the text another way, see `EditorApprovals`). That
  /// lists each stored text as it is, and doesn't canonicalize what the editor
  /// sent, so it never matches another config file that a symlink leads to.
  fn is_decisions_config_file(&self, config_file_text: &str, config_file: Option<&(String, String)>, identifier: Option<&(Uri, Option<Uri>)>) -> bool {
    let (Some((config_source, _)), Some((uri, dir_uri))) = (config_file, identifier) else {
      return true;
    };
    if config_source == config_file_text {
      return true;
    }
    let global_config_path = self.get_global_config_path();
    let Some(listed) = self.get_listed_config_file(config_file_text, global_config_path.as_deref()) else {
      return false;
    };
    let is_same_uri = |uri: &Uri, other: &Uri| match (uri_to_file_path(uri), uri_to_file_path(other)) {
      (Some(path), Some(other_path)) => path == other_path,
      _ => uri == other,
    };
    is_same_uri(&listed.uri, uri)
      && match (dir_uri, &listed.dir_uri) {
        (Some(dir_uri), Some(listed_dir_uri)) => is_same_uri(dir_uri, listed_dir_uri),
        _ => true,
      }
  }

  /// Gets the config file of a process plugin decision as `dprint/decisions`
  /// lists it from the text it's stored with, which is `None` when that isn't
  /// a path or url this version can make a uri of.
  fn get_listed_config_file(&self, config_file_text: &str, global_config_path: Option<&Path>) -> Option<ConfigFileIdentifier> {
    // it's a path unless it's a remote or npm config file
    let is_remote = ["http://", "https://", "npm:"].iter().any(|prefix| config_file_text.starts_with(prefix));
    if is_remote {
      return config_file_text.parse().ok().map(|uri| ConfigFileIdentifier { uri, dir_uri: None });
    }
    let path = Path::new(config_file_text);
    // the decisions of the global config file are for each directory it's used for
    let dir_uri = match global_config_path == Some(path) {
      true => None,
      false => get_config_file_dir(&self.environment, path).and_then(|dir_path| self.editor_uri(dir_path)),
    };
    self.editor_uri(path).map(|uri| ConfigFileIdentifier { uri, dir_uri })
  }

  /// Gets the config file a denial is for as the editor knows it now, which
  /// may not be the path it was shown with (ex. a directory it's in isn't an
  /// alias anymore, see `editor_path`). The one it was shown with is kept
  /// unless its key is the key this version makes for a local config file,
  /// so what's provided can always be forgotten (see `is_denial_for`).
  fn get_denial_config_file(&self, denied: &DeniedDownload) -> ConfigFileIdentifier {
    let (uri_text, base_path) = match denied.config_key.split_once('\n') {
      Some((uri_text, base_path)) => (uri_text, Some(Path::new(base_path))),
      None => (denied.config_key.as_str(), None),
    };
    let file_path = uri_text
      .parse::<Uri>()
      .ok()
      .filter(|uri| uri.scheme().as_str() == "file")
      .and_then(|uri| uri_to_file_path(&uri))
      .filter(|file_path| file_path_to_uri(file_path).is_some_and(|uri| uri.as_str() == uri_text));
    let current = file_path.and_then(|file_path| {
      let dir_path = match base_path {
        Some(base_path) => base_path,
        None => get_config_file_dir(&self.environment, &file_path)?,
      };
      let dir_uri = match denied.config_file.dir_uri {
        Some(_) => Some(self.editor_uri(dir_path)?),
        None => None,
      };
      Some(ConfigFileIdentifier {
        uri: self.editor_uri(&file_path)?,
        dir_uri,
      })
    });
    current.unwrap_or_else(|| denied.config_file.clone())
  }

  /// Gets an item of a download that was shown before with the editor's paths
  /// of its files now (see `editor_path`).
  fn with_current_editor_uris(&self, item: ApprovalItem, download: &str) -> ApprovalItem {
    match self.get_download_item_from_text(download) {
      Some(current) if item.npmrc_uri.is_some() || item.local_uri.is_some() => ApprovalItem {
        npmrc_uri: current.npmrc_uri,
        local_uri: current.local_uri,
        ..item
      },
      _ => item,
    }
  }

  /// Gets the item of a download from its text when what it is can be told
  /// from it (ex. a registry, the proxy of an `.npmrc` file, or a path on
  /// another machine), which is `None` for a url, because it may be a plugin
  /// or a config file. A registry's `kind` is `plugin` then, because what was
  /// downloaded from it isn't known.
  fn get_download_item_from_text(&self, download: &str) -> Option<ApprovalItem> {
    let is_url = NpmrcProxy::from_download(download).is_none() && !is_path_download(download) && get_npm_registry_download_url(download).is_none();
    if is_url {
      return None;
    }
    get_download_items(&[download.to_string()], ApprovalItemKind::Plugin, &|path| self.editor_path(path)).pop()
  }

  /// Forgets the process plugin decisions of the config files that are
  /// stored (ex. "Always Allow").
  async fn forget_stored_decisions(&self, is_config_file: &impl Fn(&str) -> bool) -> Result<()> {
    forget_editor_approvals(&self.environment, is_config_file).await
  }

  /// Gets the config file the decisions of a config file are for as the
  /// process plugin decisions have it, with its key.
  fn get_decisions_config_file(&self, uri: &Uri, dir_uri: Option<&Uri>) -> Result<(String, String)> {
    let dir_path = dir_uri.and_then(uri_to_file_path);
    let config_file = if self.scope_container.has_config_override() {
      if !self.is_config_override_uri(uri) {
        return Err(InvalidParamsError(format!("The configFile isn't the config file the server was started with: {}", uri.as_str())).into());
      }
      let dir_path = dir_path.unwrap_or_else(|| self.environment.cwd().into_path_buf());
      self.scope_container.find_config_file(&dir_path, false)?
    } else {
      // a remote or npm config file's decisions are from running dprint with it (ex. `dprint fmt --config <url>`),
      // which are by its url as it's shown
      if ["http", "https", "npm"].contains(&uri.scheme().as_str()) {
        return Ok((uri.as_str().to_string(), uri.as_str().to_string()));
      }
      let Some(file_path) = uri_to_file_path(uri) else {
        return Err(InvalidParamsError(format!("The configFile isn't a config file: {}", uri.as_str())).into());
      };
      // a decision stored for a path that isn't named like a config file (ex. by an older version, which
      // stored a symlink's canonical path) is still forgotten, without loading what's there
      if !is_config_file_path(&self.environment, &file_path) {
        let file_path = self.canonicalize_editor_path(file_path);
        let key = file_path_to_uri(&file_path).map(|uri| uri.as_str().to_string()).unwrap_or_default();
        return Ok((file_path.display().to_string(), key));
      }
      match self.load_config_file(&file_path, dir_path.as_deref()) {
        Ok(Some(config_file)) => Some(config_file),
        // a deleted config file's decisions are still forgotten
        Ok(None) => {
          let file_path = self.canonicalize_config_file_path(file_path);
          let key = file_path_to_uri(&file_path).map(|uri| uri.as_str().to_string()).unwrap_or_default();
          return Ok((file_path.display().to_string(), key));
        }
        // IMPORTANT: One on another machine isn't read or canonicalized because that contacts the
        // machine, so the decisions that are forgotten are the ones for the path it was found at
        // (ex. what's needed to read it), like the config key of the failure for it.
        Err(err) if find_config_file_on_another_machine(&err).is_some() => {
          let key = file_path_to_uri(&file_path).map(|uri| uri.as_str().to_string()).unwrap_or_default();
          return Ok((file_path.display().to_string(), key));
        }
        Err(err) => return Err(err),
      }
    };
    Ok(match config_file {
      Some(config_file) => {
        let key = get_found_config_key(&config_file).map(|(key, _)| key).unwrap_or_else(|| config_file.display());
        // without a directory, the global config file's decisions are of each directory it's used for (see `is_config_key`)
        let key = match dir_uri {
          Some(_) => key,
          None => key.split('\n').next().unwrap_or_default().to_string(),
        };
        (config_file.display(), key)
      }
      None => (String::new(), String::new()),
    })
  }

  /// Gets what needs to be approved for the scope to format files.
  fn get_scope_approvals(&self, scope: &PluginsScope<TEnvironment>, target: &ApprovalTarget) -> Vec<PendingApproval> {
    let Some(config) = scope.config.as_ref() else {
      return Vec::new();
    };
    let mut subjects = Vec::new();
    for urls in get_download_groups(&scope.untrusted_downloads) {
      let items = get_download_items(&urls, ApprovalItemKind::Plugin, &|path| self.editor_path(path));
      subjects.push((ApprovalSubject::Downloads(urls), items));
    }
    // what a process plugin that isn't set up runs with isn't known yet, so it's
    // what the trusted configuration of the config file says
    let mut trusted_configuration = None;
    if let Some(unapproved) = &scope.unapproved_process_plugins {
      let items = unapproved.plugins.iter().map(|plugin| self.get_process_plugin_item(config, plugin)).collect();
      subjects.push((ApprovalSubject::ProcessPlugins(unapproved.clone()), items));
      if unapproved.plugins.iter().any(|plugin| plugin.approval.is_none()) {
        trusted_configuration = Some(serde_json::Value::Object(
          config
            .trusted_config_map
            .iter()
            .map(|(key, value)| (key.clone(), config_map_value_to_json(value)))
            .collect(),
        ));
      }
    }
    let config_file = self.get_config_file_info(scope);
    let config_target = self.get_config_file_target(ConfigFileRef::of_config(config));
    self.store_approvals(get_config_key(config), config_file, config_target, subjects, trusted_configuration, target)
  }

  /// Gets what's run for a process plugin, which has its information and
  /// configuration when it was set up before.
  fn get_process_plugin_item(&self, config: &ResolvedConfig, unapproved_plugin: &UnapprovedProcessPlugin) -> ApprovalItem {
    let info = config
      .plugins
      .iter()
      .find(|plugin| plugin.to_full_string() == unapproved_plugin.plugin)
      .and_then(|plugin| self.scope_container.get_set_up_plugin_info(plugin));
    // only the trusted configuration is used for a process plugin
    let configuration = info
      .as_ref()
      .and_then(|info| config.trusted_config_map.get(&info.config_key))
      .map(config_map_value_to_json);
    ApprovalItem {
      specifier: escape_text(&unapproved_plugin.plugin),
      kind: ApprovalItemKind::Plugin,
      registry: unapproved_plugin.registry.as_deref().map(escape_text),
      npmrc_uri: None,
      local_uri: None,
      configuration,
      plugin: info.as_ref().map(get_file_plugin),
    }
  }

  /// Gets what needs to be approved for the config file to resolve.
  fn get_failure_approvals(&self, failure: &ConfigFailure, target: &ApprovalTarget) -> Vec<PendingApproval> {
    let config_file = failure.config_file.clone();
    let key = failure.config_key.as_ref().map(|(key, _)| key.clone()).unwrap_or_default();
    let mut subjects = Vec::new();
    // what fails to download while resolving a config file is what it extends
    for urls in get_download_groups(&failure.downloads) {
      let items = get_download_items(&urls, ApprovalItemKind::Config, &|path| self.editor_path(path));
      subjects.push((ApprovalSubject::Downloads(urls), items));
    }
    self.store_approvals(key, config_file, failure.config_target.clone(), subjects, None, target)
  }

  /// Stores what needs to be approved for the config file, which replaces what
  /// was stored for it before.
  fn store_approvals(
    &self,
    config_key: String,
    config_file: Option<ConfigFileInfo>,
    config_target: Option<ApprovalTarget>,
    subjects: Vec<(ApprovalSubject, Vec<ApprovalItem>)>,
    trusted_configuration: Option<serde_json::Value>,
    target: &ApprovalTarget,
  ) -> Vec<PendingApproval> {
    // what can't be shown with its config file isn't approved
    let Some(config_file) = config_file else {
      self.approvals.borrow_mut().remove(&config_key);
      return Vec::new();
    };
    let permissions = self.session_permissions.read(&self.environment);
    let stored = subjects
      .into_iter()
      .map(|(subject, items)| {
        let approval = match &subject {
          ApprovalSubject::Downloads(urls) => {
            let id = get_approval_id("download", &config_key, urls);
            let denied = self.is_download_denied(&config_key, urls);
            // a path that isn't valid Unicode is never allowed, so allowing it isn't offered
            let is_allowable = urls.iter().all(|url| self.environment.download_policy().is_allowable(url));
            let options: &[ApprovalDecision] = match is_allowable {
              true => &[ApprovalDecision::Allow, ApprovalDecision::Deny],
              false => &[ApprovalDecision::Deny],
            };
            let mut message = self.get_untrusted_download_message(urls);
            if !is_allowable {
              message.push_str(&format!(" {}", NOT_ALLOWABLE_PATH_MESSAGE));
            }
            PendingApproval {
              denied,
              // a download is only denied for the session
              previous_decision: denied.then_some(ApprovalDecision::Deny),
              id,
              kind: ApprovalKind::Downloads,
              title: get_download_group_title(urls).to_string(),
              config_file: config_file.clone(),
              trusted_configuration: None,
              items,
              // a download is only allowed for the session
              decisions: get_decision_options(ApprovalKind::Downloads, options),
              message,
            }
          }
          ApprovalSubject::ProcessPlugins(unapproved) => PendingApproval {
            id: get_process_plugins_approval_id(&config_key, unapproved),
            kind: ApprovalKind::ProcessPlugins,
            title: "Run process plugins".to_string(),
            config_file: config_file.clone(),
            trusted_configuration: trusted_configuration.clone(),
            items,
            decisions: get_decision_options(
              ApprovalKind::ProcessPlugins,
              &[
                ApprovalDecision::Allow,
                ApprovalDecision::AlwaysAllow,
                ApprovalDecision::Deny,
                ApprovalDecision::AlwaysDeny,
              ],
            ),
            denied: unapproved.is_denied(&permissions),
            previous_decision: match unapproved.is_denied(&permissions) {
              true if unapproved.is_always_denied(&permissions) => Some(ApprovalDecision::AlwaysDeny),
              true => Some(ApprovalDecision::Deny),
              false => None,
            },
            message: unapproved.message(&permissions),
          },
        };
        StoredApproval {
          config_key: config_key.clone(),
          config_target: config_target.clone().unwrap_or_else(|| target.clone()),
          approval,
          subject,
        }
      })
      .collect::<Vec<_>>();
    let approvals = stored.iter().map(|stored| stored.approval.clone()).collect();
    let mut approval_files = self.approval_files.borrow_mut();
    for stored in &stored {
      let downloads = match &stored.subject {
        ApprovalSubject::Downloads(urls) => Some(urls.clone()),
        ApprovalSubject::ProcessPlugins(_) => None,
      };
      let provided = ProvidedApproval {
        target: stored.config_target.clone(),
        downloads,
      };
      approval_files.insert(stored.approval.id.clone(), provided);
    }
    while approval_files.len() > MAX_APPROVAL_FILES {
      approval_files.shift_remove_index(0);
    }
    drop(approval_files);
    if stored.is_empty() {
      self.approvals.borrow_mut().remove(&config_key);
    } else {
      self.approvals.borrow_mut().insert(config_key, stored);
    }
    approvals
  }

  /// Asks a language client's user to approve what's pending, which is only
  /// done once per session for the same thing and not when they denied it.
  /// The next format uses the config file once it's approved.
  fn prompt_for_approvals(self: &Rc<Self>, approvals: &[PendingApproval]) {
    let ClientKind::LanguageClient { supports_message_actions } = self.client_kind.get() else {
      // an editor extension asks the user itself
      return;
    };
    let mut pending = self.take_unprompted_approvals(approvals.iter().cloned());
    if !supports_message_actions {
      for approval in pending {
        let guidance = match approval.kind {
          ApprovalKind::Downloads => {
            "The client can't ask whether to download them, so run `dprint config install` in the directory of the configuration file to do so.".to_string()
          }
          ApprovalKind::ProcessPlugins => self
            .approvals
            .borrow()
            .values()
            .flatten()
            .find_map(|stored| match &stored.subject {
              ApprovalSubject::ProcessPlugins(unapproved) if stored.approval.id == approval.id => {
                Some(get_cant_ask_message(&self.environment, &unapproved.config_file))
              }
              _ => None,
            })
            .unwrap_or_default(),
        };
        self.client.show_message(MessageType::WARNING, format!("{} {}", approval.message, guidance));
      }
      return;
    }
    if pending.is_empty() {
      return;
    }
    let workspace = self.clone();
    // don't wait on this because it only resolves once the user responds
    dprint_core::async_runtime::spawn(async move {
      while !pending.is_empty() {
        let approval = pending.remove(0);
        let titles = match approval.kind {
          // a path that isn't valid Unicode can only be denied (see `DownloadPolicy::is_allowable`)
          ApprovalKind::Downloads if !approval.decisions.iter().any(|option| option.decision == ApprovalDecision::Allow) => {
            vec![CANCEL_DOWNLOAD_ACTION_TITLE]
          }
          ApprovalKind::Downloads => vec![DOWNLOAD_ACTION_TITLE, CANCEL_DOWNLOAD_ACTION_TITLE],
          ApprovalKind::ProcessPlugins => PROCESS_PLUGIN_ACTION_TITLES.to_vec(),
        };
        let message = get_language_client_approval_message(&approval);
        let Some(title) = workspace.show_message_request(MessageType::WARNING, message, &titles).await else {
          // not choosing an action leaves it undecided
          return;
        };
        let decision = match approval.kind {
          ApprovalKind::Downloads if title == DOWNLOAD_ACTION_TITLE => ApprovalDecision::Allow,
          ApprovalKind::Downloads => ApprovalDecision::Deny,
          ApprovalKind::ProcessPlugins => match get_process_plugin_decision(&title) {
            Some(decision) => decision,
            // the client responded with an action that wasn't offered
            None => return,
          },
        };
        // the user may have responded a while after the task that asked them
        workspace.check_editor_dirs_again();
        match workspace.approve(&approval.id, decision).await {
          Ok(result) => {
            // a language client's user is told with a message
            if let Some(notice) = result.notice {
              workspace.client.show_message(MessageType::WARNING, notice.message);
            }
            pending.extend(workspace.take_unprompted_approvals(result.config_approvals.approvals.into_iter()));
          }
          Err(err) => {
            let message = format!("Failed setting up what was allowed: {:#}", err);
            log_error!(workspace.environment, "{}", message);
            workspace.client.log_error(message);
          }
        }
      }
    });
  }

  /// Gets the approvals that a language client's user wasn't asked about,
  /// which they're asked about once.
  fn take_unprompted_approvals(&self, approvals: impl Iterator<Item = PendingApproval>) -> Vec<PendingApproval> {
    approvals
      .filter(|approval| !approval.denied && self.prompted_approvals.borrow_mut().insert(approval.id.clone()))
      .collect()
  }

  /// Gets the outcome of a file that wasn't formatted because of problems with
  /// the configuration of a plugin, which are also shown to a language
  /// client's user once for the config file until it changes because
  /// formatting doesn't log them again.
  fn get_plugin_config_diagnostics_outcome(&self, scope: &Rc<PluginsScope<TEnvironment>>, diagnostics: &[PluginConfigDiagnostic]) -> FormatOutcome {
    let config_file = scope.config.as_ref().map(|config| config.display()).unwrap_or_default();
    let mut message = format!("dprint did not format the file because of errors in the configuration of {}:", config_file);
    for diagnostic in diagnostics {
      message.push('\n');
      message.push_str(&diagnostic.to_string());
    }
    let shown = ShownPluginConfigError {
      scope: Rc::downgrade(scope),
      message: message.clone(),
    };
    let previous = self.plugin_config_errors.borrow_mut().insert(config_file, shown);
    let was_shown = previous.is_some_and(|previous| previous.scope.ptr_eq(&Rc::downgrade(scope)) && previous.message == message);
    if !was_shown {
      self.show_error(message.clone());
    }
    // the diagnostics are where they are in the config file's text
    let config_text = self
      .checked_scopes
      .borrow()
      .values()
      .find(|checked| checked.scope.ptr_eq(&Rc::downgrade(scope)))
      .map(|checked| checked.config_text.clone());
    let plugin_diagnostics = diagnostics
      .iter()
      .map(|diagnostic| PluginSectionDiagnostic {
        config_key: scope
          .plugins
          .values()
          .find(|plugin| plugin.name() == diagnostic.plugin_name)
          .map(|plugin| plugin.info().config_key.as_str())
          .unwrap_or_default(),
        diagnostic,
      })
      .collect::<Vec<_>>();
    if let Some(config) = &scope.config {
      self.publish_config_diagnostics(
        ConfigFileRef::of_config(config),
        get_config_diagnostics(config_text.as_deref(), scope.global_config_diagnostics(), &plugin_diagnostics),
      );
    }
    // one that may not be read now isn't offered to be opened (see `publish_config_diagnostics`)
    let actions = scope
      .config
      .as_ref()
      .filter(|config| config.file_path().is_none_or(|path| self.allows_reading_path(path)))
      .and_then(|config| self.editor_config_file_uri(ConfigFileRef::of_config(config)))
      .map(|uri| vec![open_config_file_action(&uri)])
      .unwrap_or_default();
    FormatOutcome::NotFormatted(NotFormatted::new(NotFormattedReason::ConfigDiagnostics, Some(message), Vec::new()).with_actions(actions))
  }

  /// Notifies a language client's user that a file wasn't formatted because no
  /// config file was found for it. This is only done once per session to not
  /// annoy people.
  fn notify_no_config(self: &Rc<Self>, reason: NotFormattedReason, use_global_config: bool) {
    let ClientKind::LanguageClient { supports_message_actions } = self.client_kind.get() else {
      // an editor extension notifies the user itself
      return;
    };
    let workspace_folders = {
      let settings = self.settings.borrow();
      if !settings.show_no_config_notification || self.has_notified_no_config.get() {
        return;
      }
      settings.workspace_folders.clone()
    };
    // the workspace folders may change to ones where it wasn't dismissed
    if is_no_config_notification_dismissed(&self.environment, &workspace_folders) {
      return;
    }
    self.has_notified_no_config.set(true);
    let message = get_no_config_message(NoConfigMessageOptions {
      use_global_config,
      has_global_config: reason == NotFormattedReason::GlobalConfigNotAllowed,
      settings_owner: self.get_settings_owner(),
    });
    // the notification can only be dismissed for good in a client that shows the actions
    if !supports_message_actions {
      self.client.show_message(MessageType::INFO, message);
      return;
    }
    let mut actions = Vec::new();
    if !workspace_folders.is_empty() {
      actions.push(DISMISS_WORKSPACE_ACTION_TITLE);
    }
    actions.push(DISMISS_ACTION_TITLE);
    let workspace = self.clone();
    // don't wait on this because it only resolves once the user responds
    dprint_core::async_runtime::spawn(async move {
      let Some(title) = workspace.show_message_request(MessageType::INFO, message, &actions).await else {
        return;
      };
      let dismissal = match title.as_str() {
        DISMISS_WORKSPACE_ACTION_TITLE => NoConfigNotificationDismissal::WorkspaceFolders(workspace_folders),
        DISMISS_ACTION_TITLE => NoConfigNotificationDismissal::Everywhere,
        // the client responded with an action that wasn't offered
        _ => return,
      };
      if let Err(err) = dismiss_no_config_notification(&workspace.environment, dismissal).await {
        let message = format!("Failed storing to not show the no configuration file notification: {:#}", err);
        log_warn!(workspace.environment, "{}", message);
        workspace.client.log_warn(message);
      }
    });
  }

  /// Shows the message to the user with the actions to choose from, which
  /// resolves to the title of the one they chose.
  async fn show_message_request(&self, message_type: MessageType, message: String, titles: &[&str]) -> Option<String> {
    let actions = titles
      .iter()
      .map(|title| deno_tower_lsp::lsp_types::MessageActionItem {
        title: title.to_string(),
        properties: Default::default(),
      })
      .collect();
    let selection = self.client.show_message_request(message_type, message, actions).await?;
    Some(selection.title)
  }

  /// Resolves the plugins of the config file used for the file.
  async fn resolve_scope(self: &Rc<Self>, file_path: &Path, use_global_config: bool) -> Result<DocumentConfig<TEnvironment>> {
    let Some(dir_path) = file_path.parent() else {
      return Err(UnsupportedDocumentError::unsupported(format!(
        "Cannot format non-file path: {}",
        file_path.display()
      )));
    };
    let config_file = match self.scope_container.find_config_file(dir_path, use_global_config) {
      Ok(config_file) => {
        self.config_find_errors.borrow_mut().remove(dir_path);
        config_file
      }
      Err(err) => {
        // not an error because it's found once the user allows reading it from the other machine
        if let Some(err) = find_config_file_on_another_machine(&err) {
          self.config_find_errors.borrow_mut().remove(dir_path);
          return Ok(DocumentConfig::Failed(self.new_config_file_on_another_machine_failure(err)));
        }
        self.show_config_find_error(dir_path, &err);
        return Ok(DocumentConfig::Failed(ConfigFailure {
          config_file: None,
          config_key: None,
          config_target: None,
          reason: NotFormattedReason::ConfigError,
          message: format!("{:#}", err),
          downloads: Vec::new(),
        }));
      }
    };
    let Some(config_file) = config_file else {
      return Ok(DocumentConfig::NotFound);
    };
    self.resolve_found_config(config_file).await
  }

  /// Gets the failure of a config file that was found on another machine,
  /// which is used once the user allows reading it.
  fn new_config_file_on_another_machine_failure(&self, err: &ConfigFileOnAnotherMachineError) -> ConfigFailure {
    // what it formats changed when it was resolved before (ex. a `git pull` replaced it with a symlink to a
    // share), which the editor is told like when it fails to resolve, and it's forgotten so that it isn't
    // resolved again before reading it is allowed (see `resolve_used_configs_again`)
    // it's known by the path that identifies it, which is only canonicalized once reading it is allowed
    let file_path = self.canonicalize_config_file_path(err.config_path.clone());
    // IMPORTANT: The editor doesn't open its path from the problems that were published for it then, which would
    // read it from the other machine (ex. a `git pull` replaced it with a symlink to a share).
    self.clear_config_diagnostics(&file_path);
    if self.was_resolved(&file_path) {
      self.forget_config_file(&file_path);
      self.send_file_config_changed(&file_path, ConfigChange::Changed);
    }
    let downloads = vec![err.download.download.clone()];
    let config_file = self
      .editor_uri(&err.config_path)
      .zip(self.editor_uri(&err.dir))
      .map(|(uri, dir_uri)| ConfigFileInfo {
        uri,
        dir_uri,
        kind: self.get_config_file_kind(false),
        in_workspace: self.is_in_workspace(&err.config_path),
        // IMPORTANT: Opening it before it's allowed reads it from the other machine.
        on_another_machine: true,
      });
    ConfigFailure {
      config_file,
      // it's the path it was found at because it isn't canonicalized until it's read
      config_key: file_path_to_uri(&err.config_path).map(|uri| (uri.as_str().to_string(), uri)),
      // it's found again for the file once it's allowed
      config_target: None,
      reason: NotFormattedReason::ApprovalRequired,
      message: self.get_untrusted_download_message(&downloads),
      downloads,
    }
  }

  /// Resolves the plugins of a config file that was found.
  async fn resolve_found_config(self: &Rc<Self>, config_file: ResolvedConfigPathWithText) -> Result<DocumentConfig<TEnvironment>> {
    let key = get_config_file_key(&config_file);
    self.used_configs.borrow_mut().insert(key.clone(), config_file.clone());
    let failed = |reason: NotFormattedReason, message: String, downloads: Vec<String>| {
      DocumentConfig::Failed(ConfigFailure {
        config_file: self.new_config_file_info(ConfigFileRef::of_found(&config_file)),
        config_key: get_found_config_key(&config_file),
        config_target: self.get_config_file_target(ConfigFileRef::of_found(&config_file)),
        reason,
        message,
        downloads,
      })
    };
    let scope = match self.scope_container.resolve_config_file(&config_file).await {
      Ok(scope) => scope,
      Err(err) => {
        self.failed_configs.borrow_mut().insert(key.clone());
        // what it formats changed when it resolved before
        if self.resolved_configs.borrow_mut().remove(&key).is_some()
          && let Some(params) = self.new_found_config_changed(&config_file)
        {
          self.send_config_changed(params);
        }
        // not an error because it resolves once the user allows downloading what it extends
        if let Some(err) = UntrustedDownloadError::find(&err) {
          // ex. a file it extends that's on another machine now, whose directory isn't watched then
          self.register_watch_files();
          let downloads = vec![err.download.clone()];
          return Ok(failed(
            NotFormattedReason::ApprovalRequired,
            self.get_untrusted_download_message(&downloads),
            downloads,
          ));
        }
        self.show_config_error(key.clone(), &err);
        self.checked_scopes.borrow_mut().remove(&key);
        self.publish_config_diagnostics(
          ConfigFileRef::of_found(&config_file),
          vec![get_config_error_diagnostic(Some(&config_file.content), &err)],
        );
        return Ok(failed(NotFormattedReason::ConfigError, format!("{:#}", err), Vec::new()));
      }
    };
    self.check_config_diagnostics(&key, &config_file, &scope);
    if let Err(err) = scope.ensure_no_global_config_diagnostics() {
      let err = anyhow::Error::from(err);
      self.failed_configs.borrow_mut().insert(key.clone());
      // what it formats changed when it resolved before, like for an error resolving it
      if self.resolved_configs.borrow_mut().remove(&key).is_some()
        && let Some(params) = self.new_found_config_changed(&config_file)
      {
        self.send_config_changed(params);
      }
      self.show_config_error(key, &err);
      return Ok(failed(NotFormattedReason::ConfigError, format!("{:#}", err), Vec::new()));
    }
    self.config_errors.borrow_mut().remove(&key);
    let was_failing = self.failed_configs.borrow_mut().remove(&key);
    self.notify_if_config_changed(key, &config_file, &scope, was_failing);
    Ok(DocumentConfig::Resolved(scope))
  }

  /// Publishes the problems with the config file, which is done once for each
  /// time it resolves to a new scope. The problems with the configuration of
  /// its plugins are only known once they're set up, so they're published
  /// again once that's done.
  fn check_config_diagnostics(self: &Rc<Self>, key: &str, config_file: &ResolvedConfigPathWithText, scope: &Rc<PluginsScope<TEnvironment>>) {
    let weak_scope = Rc::downgrade(scope);
    let config_text: Rc<str> = config_file.content.as_str().into();
    let checked_scope = CheckedScope {
      scope: weak_scope.clone(),
      config_text: config_text.clone(),
    };
    let previous = self.checked_scopes.borrow_mut().insert(key.to_string(), checked_scope);
    if previous.is_some_and(|previous| previous.scope.ptr_eq(&weak_scope)) {
      return;
    }
    self.publish_config_diagnostics(
      ConfigFileRef::of_found(config_file),
      get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &[]),
    );
    let workspace = self.clone();
    let key = key.to_string();
    let config_file = config_file.clone();
    let scope = scope.clone();
    dprint_core::async_runtime::spawn(async move {
      let mut plugin_diagnostics = Vec::new();
      for plugin in scope.plugins.values() {
        // a plugin that fails to be set up is reported when it formats
        if let Ok(GetPluginResult::HadDiagnostics(diagnostics)) = plugin.get_or_create_checking_config_diagnostics(&workspace.environment).await {
          plugin_diagnostics.extend(diagnostics.into_iter().map(|diagnostic| (plugin.info().config_key.clone(), diagnostic)));
        }
      }
      if plugin_diagnostics.is_empty() {
        return;
      }
      // the config file may have changed in the meantime
      let is_current = workspace
        .checked_scopes
        .borrow()
        .get(&key)
        .is_some_and(|checked| checked.scope.ptr_eq(&Rc::downgrade(&scope)));
      if !is_current {
        return;
      }
      let plugin_diagnostics = plugin_diagnostics
        .iter()
        .map(|(config_key, diagnostic)| PluginSectionDiagnostic { config_key, diagnostic })
        .collect::<Vec<_>>();
      workspace.check_editor_dirs_again();
      workspace.publish_config_diagnostics(
        ConfigFileRef::of_found(&config_file),
        get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &plugin_diagnostics),
      );
    });
  }

  /// Publishes the problems with the config file when they're different from
  /// the ones that were published before.
  fn publish_config_diagnostics(&self, config_file: ConfigFileRef<'_>, diagnostics: Vec<ConfigDiagnostic>) {
    // IMPORTANT: One that may not be read now (ex. a `git pull` replaced it with a symlink to a share after it was
    // resolved) has its problems taken back instead, because the editor would open it from them.
    if config_file.file_path.is_some_and(|path| !self.allows_reading_path(path)) {
      if let Some(file_path) = config_file.file_path {
        self.clear_config_diagnostics(file_path);
      }
      return;
    }
    if let (Some(uri), Some(editor_uri)) = (config_file.uri(), self.editor_config_file_uri(config_file)) {
      self.publish_diagnostics(&uri, editor_uri, diagnostics);
    }
  }

  /// Takes back the problems that were published for the config file at the
  /// path that identifies it.
  fn clear_config_diagnostics(&self, file_path: &Path) {
    let Some(uri) = file_path_to_uri(file_path) else {
      return;
    };
    let previous = self.published_diagnostics.borrow_mut().remove(uri.as_str());
    if let Some((editor_uri, _)) = previous {
      self.client.publish_diagnostics(editor_uri, Vec::new());
    }
  }

  /// Takes back the problems that were published to the editor's uris in the
  /// directory of an alias that isn't one anymore (see `editor_path`), and
  /// tells an editor extension that their config files changed, so it gets
  /// their results again.
  ///
  /// IMPORTANT: They aren't published to another uri now, because what that
  /// leads to isn't known until the config file is resolved again (ex. it may
  /// be on another machine now too), which publishes them again.
  fn take_back_diagnostics_under(&self, editor_dir: &Path) {
    let taken_back = {
      let mut published_diagnostics = self.published_diagnostics.borrow_mut();
      let keys = published_diagnostics
        .iter()
        .filter(|(_, (editor_uri, _))| uri_to_file_path(editor_uri).is_some_and(|path| path.starts_with(editor_dir)))
        .map(|(uri, _)| uri.clone())
        .collect::<Vec<_>>();
      keys
        .into_iter()
        .filter_map(|uri| Some((uri.clone(), published_diagnostics.remove(&uri)?.0)))
        .collect::<Vec<_>>()
    };
    for (uri, editor_uri) in taken_back {
      self.client.publish_diagnostics(editor_uri.clone(), Vec::new());
      // they're published again when it's resolved next
      if let Some(file_path) = uri.parse::<Uri>().ok().as_ref().and_then(uri_to_file_path) {
        let is_config_file = |key: &String| key.split('\n').next().is_some_and(|source| Path::new(source) == file_path);
        self.checked_scopes.borrow_mut().retain(|key, _| !is_config_file(key));
      }
      if let Some(editor_path) = uri_to_file_path(&editor_uri)
        && let Some(dir_uri) = get_config_file_dir(&self.environment, &editor_path).and_then(file_path_to_uri)
      {
        self.send_config_changed(new_identified_config_changed(editor_uri, dir_uri, ConfigChange::Changed));
      }
    }
  }

  /// Publishes the problems with the config file of the uri that identifies it
  /// to the editor's uri of it, which clears the ones published to another
  /// editor's uri of it before.
  fn publish_diagnostics(&self, uri: &Uri, editor_uri: Uri, diagnostics: Vec<ConfigDiagnostic>) {
    let mut published_diagnostics = self.published_diagnostics.borrow_mut();
    let previous = published_diagnostics.get(uri.as_str());
    // there's nothing to clear when nothing was published
    let is_same_uri = previous.is_none_or(|(previous_uri, _)| *previous_uri == editor_uri);
    if is_same_uri && (previous.map(|(_, previous)| previous) == Some(&diagnostics) || previous.is_none() && diagnostics.is_empty()) {
      return;
    }
    let previous_uri = published_diagnostics
      .insert(uri.as_str().to_string(), (editor_uri.clone(), diagnostics.clone()))
      .map(|(previous_uri, _)| previous_uri)
      .filter(|previous_uri| *previous_uri != editor_uri);
    drop(published_diagnostics);
    if let Some(previous_uri) = previous_uri {
      self.client.publish_diagnostics(previous_uri, Vec::new());
    }
    self
      .client
      .publish_diagnostics(editor_uri, diagnostics.into_iter().map(|diagnostic| diagnostic.into_lsp_diagnostic()).collect());
  }

  /// Tells an editor extension when the config file resolves to a different
  /// config than before because what it formats may have changed in a way the
  /// editor can't see (ex. a config file that it extends changed or what
  /// needed to be approved was approved).
  fn notify_if_config_changed(&self, key: String, config_file: &ResolvedConfigPathWithText, scope: &PluginsScope<TEnvironment>, was_failing: bool) {
    let Some(config) = &scope.config else {
      return;
    };
    let mut are_dependencies_changed = false;
    if let (Some(uri), Some(config_file_path)) = (ConfigFileRef::of_found(config_file).uri(), config_file.file_path()) {
      let dependencies = ConfigDependencies {
        file_path: config_file_path.to_path_buf(),
        canonical_path: get_other_canonical_path(&config_file.source, config_file.found_path.as_deref()).map(|path| path.to_path_buf()),
        dependencies: config.local_dependencies.clone(),
      };
      let previous = self.config_dependencies.borrow_mut().insert(uri.as_str().to_string(), dependencies.clone());
      are_dependencies_changed = previous.as_ref() != Some(&dependencies);
    }
    let state = ResolvedConfigState {
      config: config.clone(),
      untrusted_downloads: scope.untrusted_downloads.clone(),
      process_plugins_allowed: scope.unapproved_process_plugins.is_none(),
      denied: self.get_scope_denials(scope),
    };
    let previous = self.resolved_configs.borrow_mut().insert(key, state.clone());
    // it resolves again after it failed (ex. what it extends was fixed or allowed to be downloaded)
    if (was_failing || previous.is_some_and(|previous| previous != state))
      && let Some(params) = self.new_found_config_changed(config_file)
    {
      self.send_config_changed(params);
    }
    // registering them follows the path of each directory (see `register_watch_files`)
    if are_dependencies_changed {
      self.register_watch_files();
    }
  }

  /// Tells the download policy what the user denied downloading this session,
  /// which an allowed npm registry doesn't allow then (see
  /// `DownloadPolicy::set_denied_downloads`).
  fn denied_downloads_changed(&self) {
    let downloads = self
      .denied_downloads
      .borrow()
      .values()
      .flat_map(|denied| denied.downloads.clone())
      .collect::<Vec<_>>();
    self.environment.download_policy().set_denied_downloads(downloads);
  }

  /// Gets whether the user denied downloading the downloads for the config
  /// file this session, which includes a denial of more of them (ex. one of
  /// them was allowed for another config file since), so what they denied
  /// isn't asked about again.
  fn is_download_denied(&self, config_key: &str, downloads: &[String]) -> bool {
    self
      .denied_downloads
      .borrow()
      .values()
      .any(|denied| denied.config_key == config_key && downloads.iter().all(|download| denied.downloads.contains(download)))
  }

  /// Gets whether the download and the process plugins that need to be
  /// approved for the scope were denied.
  fn get_scope_denials(&self, scope: &PluginsScope<TEnvironment>) -> (bool, bool) {
    let Some(config) = &scope.config else {
      return (false, false);
    };
    let config_key = get_config_key(config);
    let is_download_denied = get_download_groups(&scope.untrusted_downloads)
      .iter()
      .any(|urls| self.is_download_denied(&config_key, urls));
    let is_process_plugins_denied = scope
      .unapproved_process_plugins
      .as_ref()
      .is_some_and(|unapproved| unapproved.is_denied(&self.session_permissions.read(&self.environment)));
    (is_download_denied, is_process_plugins_denied)
  }

  /// Asks the client to watch the config files that were resolved and the
  /// files they were resolved from when it uses the `watchFiles` feature,
  /// which is done again when they change. The config files are also watched
  /// by a glob that's registered once the client is initialized (ex. for one
  /// that's created), and the global config file is watched by the server.
  fn register_watch_files(&self) {
    let mut registered = self.registered_watch_files.borrow_mut();
    let Some(registered) = registered.as_mut() else {
      return;
    };
    let global_config_path = self.get_global_config_path();
    let mut file_paths = Vec::new();
    for dependencies in self.config_dependencies.borrow().values() {
      let config_file_path = &dependencies.file_path;
      for file_path in std::iter::once(config_file_path)
        .chain(dependencies.canonical_path.iter())
        .chain(dependencies.dependencies.iter())
      {
        if Some(file_path) != global_config_path.as_ref() && !file_paths.contains(file_path) {
          file_paths.push(file_path.clone());
        }
      }
    }
    // IMPORTANT: Each is watched by its canonical directory, so it isn't a symlink (ex. the path a config file in a
    // `.config` directory that's a symlink was found at is watched as what it points to), and one in a directory
    // that may not be read is left out because watching it would follow it to another machine (ex. a directory a
    // `git pull` replaced with a symlink to a share). A watcher of a directory doesn't follow the symlinks in it (see
    // "Lifecycle" in the editor extension docs), so a file that's a symlink is fine.
    let mut canonical_dirs = HashMap::new();
    let mut file_paths = file_paths
      .into_iter()
      .filter_map(|path| {
        let (dir_path, file_name) = (path.parent()?, path.file_name()?);
        let canonical_dir = canonical_dirs.entry(dir_path.to_path_buf()).or_insert_with(|| {
          self
            .allows_reading_path(dir_path)
            .then(|| self.environment.canonicalize_maybe_not_exists(dir_path).ok())
            .flatten()
        });
        Some(canonical_dir.as_ref()?.as_ref().join(file_name))
      })
      .collect::<Vec<_>>();
    file_paths.sort();
    file_paths.dedup();
    if *registered != file_paths {
      self.client.register_watched_files(&file_paths);
      *registered = file_paths;
    }
  }

  /// Gets the uris of the files a config file was resolved from for the
  /// editor to watch (see `get_watch_uri`), which start with the canonical
  /// path of a local config file (the path that identifies it with its
  /// canonical path) when the editor's watcher of the config files doesn't
  /// report a change there, which is when it's a symlink (or is in a `.config`
  /// directory that's one), and when the editor's path of it is through a
  /// directory symlink it doesn't watch through.
  fn get_watch_uris(&self, config_file: Option<(&Path, &Path)>, dependencies: &[PathBuf]) -> Vec<Uri> {
    let canonical_path = config_file
      .filter(|(file_path, canonical_path)| file_path != canonical_path || !self.is_watchable_editor_path(file_path))
      .map(|(_, canonical_path)| canonical_path);
    canonical_path
      .into_iter()
      .chain(dependencies.iter().map(|path| path.as_path()))
      .filter_map(|path| self.get_watch_uri(path))
      .collect()
  }

  /// Gets the uris to watch of a config file that doesn't exist, which are
  /// what it points to when it's a dangling symlink, so its being created
  /// again is reported. That's what it was resolved from the last time it
  /// resolved when this server resolved it before.
  fn get_dangling_symlink_watch_uris(&self, file_path: &Path) -> Vec<Uri> {
    let file_path = self.canonicalize_config_file_path(file_path.to_path_buf());
    let dependencies = file_path_to_uri(&file_path).and_then(|uri| self.config_dependencies.borrow().get(uri.as_str()).cloned());
    if let Some(dependencies) = dependencies {
      return self.get_dependencies_watch_uris(&dependencies);
    }
    // reading the link doesn't follow it, and its directory was checked when it was looked up
    let Some(dir_path) = file_path.parent().filter(|dir_path| self.allows_reading_path(dir_path)) else {
      return Vec::new();
    };
    let target = match self.environment.read_link(&file_path) {
      Ok(target) => self.canonicalize_editor_path(dir_path.join(target)),
      // what a path that doesn't exist canonicalizes to otherwise (ex. a symlink that canonicalizing resolves)
      Err(_) => self.canonicalize_editor_path(file_path.clone()),
    };
    match target != file_path {
      true => self.get_watch_uri(&target).into_iter().collect(),
      false => Vec::new(),
    }
  }

  /// Gets the uris to watch of what a config file was resolved from the last
  /// time it resolved (see `get_watch_uris`).
  fn get_dependencies_watch_uris(&self, dependencies: &ConfigDependencies) -> Vec<Uri> {
    let canonical_path = dependencies.canonical_path.as_deref().unwrap_or(&dependencies.file_path);
    self.get_watch_uris(Some((&dependencies.file_path, canonical_path)), &dependencies.dependencies)
  }

  /// Gets the uri of a file a config file was resolved from for the editor to
  /// watch, which is `None` when its directory may not be read (see
  /// `register_watch_files`), so the editor doesn't watch it.
  ///
  /// It's the editor's path when that's through a workspace folder or one of
  /// its ancestors that the editor knows by another path, and otherwise the
  /// canonical path, because the editor only watches through a symlink that's
  /// a workspace folder or one of its ancestors (see "Lifecycle" in the editor
  /// extension docs), so it wouldn't watch a file through another one (ex. a
  /// `.config` directory that's a symlink, or an npm package that pnpm links).
  fn get_watch_uri(&self, path: &Path) -> Option<Uri> {
    if !path.parent().is_some_and(|dir_path| self.allows_reading_path(dir_path)) {
      return None;
    }
    match self.is_watchable_editor_path(path) {
      true => self.editor_uri(path),
      false => file_path_to_uri(path),
    }
  }

  /// Gets whether the editor watches the canonical path by its path of it,
  /// which it doesn't when that's through a symlink that isn't a workspace
  /// folder or one of its ancestors (see `get_watch_uri`).
  fn is_watchable_editor_path(&self, path: &Path) -> bool {
    // the alias is checked first (see `editor_path`)
    self.editor_path(path);
    match self.editor_paths.get_alias(path) {
      Some((_, editor_dir)) => self.settings.borrow().workspace_folders.iter().any(|folder| folder.starts_with(&editor_dir)),
      None => true,
    }
  }

  /// Tells an editor extension that what a config file formats may have changed.
  fn send_config_changed(&self, params: ConfigChangedParams) {
    if self.client_kind.get() == ClientKind::EditorExtension {
      self.client.send_config_changed(params);
    }
  }

  /// Watches dprint's own files, which are outside the workspace and change
  /// what the config files resolve to (ex. adding a trusted domain in the
  /// settings file, allowing a process plugin by running `dprint fmt`, or
  /// editing the global config file). The editor isn't asked to watch them
  /// because their location depends on the system. They're checked
  /// periodically instead when they can't be watched.
  fn watch_dprint_files(self: &Rc<Self>) {
    let Some(settings_file_path) = get_settings_file_path(&self.environment) else {
      return;
    };
    self.had_global_config.set(self.get_global_config_path().is_some());
    let mut file_paths = vec![settings_file_path.clone()];
    file_paths.extend(get_editor_approvals_file_path(&self.environment));
    // the global config file is in another directory when DPRINT_CONFIG_DIR can't be used
    let config_dirs = settings_file_path
      .parent()
      .map(|dir| dir.to_path_buf())
      .into_iter()
      .chain(resolve_global_config_dir(&self.environment).ok().map(|dir| dir.into_path_buf()));
    for config_dir in config_dirs {
      for name in ["dprint.jsonc", "dprint.json"] {
        let file_path = config_dir.join(name);
        if !file_paths.contains(&file_path) {
          file_paths.push(file_path);
        }
      }
    }
    // the files are only checked periodically when they can't be watched (ex. their directory doesn't exist)
    let mut watch_paths = get_dprint_files_watch_paths(&self.environment, &file_paths);
    let mut watcher = match self.environment.watch_files(&watch_paths) {
      Ok(watcher) => Some(watcher),
      Err(err) => {
        log_debug!(
          self.environment,
          "Checking dprint's own files periodically because they can't be watched: {:#}",
          err
        );
        None
      }
    };
    let workspace = Rc::downgrade(self);
    let stopped = self.stopped.clone();
    dprint_core::async_runtime::spawn(async move {
      // a file that can't be read right now (ex. while it's written) keeps its state
      let read_states = |environment: &TEnvironment, previous: Option<&Vec<Option<u64>>>| {
        file_paths
          .iter()
          .enumerate()
          .map(|(index, file_path)| match environment.maybe_read_file(file_path) {
            Ok(text) => text.map(|text| get_bytes_hash(text.as_bytes())),
            Err(_) => previous.and_then(|previous| previous[index]),
          })
          .collect::<Vec<_>>()
      };
      let mut states = match workspace.upgrade() {
        Some(workspace) => read_states(&workspace.environment, None),
        None => return,
      };
      loop {
        match &mut watcher {
          Some(files_watcher) => {
            tokio::select! {
              change = files_watcher.changes.recv() => {
                if change.is_none() {
                  // it stopped watching, so they're checked periodically instead
                  watcher = None;
                  continue;
                }
              }
              _ = stopped.cancelled() => return,
            }
            // a change may be several events (ex. an atomic write), which are handled together
            tokio::time::sleep(DPRINT_FILES_CHANGE_DELAY).await;
            while files_watcher.changes.try_recv().is_ok() {}
          }
          None => {
            tokio::select! {
              _ = tokio::time::sleep(DPRINT_FILES_POLL_INTERVAL) => {}
              _ = stopped.cancelled() => return,
            }
          }
        }
        let Some(workspace) = workspace.upgrade() else {
          return;
        };
        // they're watched once they can be (ex. the directory was created), and again when one that's a
        // symlink points elsewhere now
        let new_watch_paths = get_dprint_files_watch_paths(&workspace.environment, &file_paths);
        if watcher.is_none() || new_watch_paths != watch_paths {
          watcher = workspace.environment.watch_files(&new_watch_paths).ok();
          watch_paths = new_watch_paths;
        }
        // only a change to what's in them matters (ex. not a write of the same text)
        let new_states = read_states(&workspace.environment, Some(&states));
        if new_states != states {
          states = new_states;
          log_debug!(workspace.environment, "dprint's settings, approvals, or global config file changed.");
          workspace.handle_dprint_files_change().await;
        }
      }
    });
  }

  /// Resolves the config files that were used again after dprint's own files
  /// changed, which tells an editor extension about the ones whose results
  /// changed (ex. a domain that was trusted lets a plugin be downloaded).
  async fn handle_dprint_files_change(self: &Rc<Self>) {
    self.check_editor_dirs_again();
    self.environment.download_policy().reload_trusted_domains(&self.environment);
    self.resolve_used_configs_again().await;
    // the documents without a config file may use the global config file now
    let has_global_config = self.get_global_config_path().is_some();
    if self.had_global_config.replace(has_global_config) != has_global_config {
      self.send_config_changed(new_every_config_changed());
    }
  }

  /// Resolves the config files that were used again, which tells an editor
  /// extension about the ones whose results changed.
  async fn resolve_used_configs_again(self: &Rc<Self>) {
    let config_files = self.used_configs.borrow().values().cloned().collect::<Vec<_>>();
    for previous in config_files {
      if self.stopped.is_cancelled() {
        return;
      }
      // what's provided for each is checked again, because resolving one may take a while (ex. a download)
      self.check_editor_dirs_again();
      // read it again in case it changed (ex. the global config file)
      let config_file = if previous.is_global_config {
        resolve_global_config_path_and_text_at_path_root(&self.environment, previous.base_path.as_ref())
          .ok()
          .flatten()
      } else {
        let Some(file_path) = previous.file_path() else {
          continue;
        };
        // IMPORTANT: It's read like a config file from the editor, because it may be on another machine now
        // (ex. a `git pull` replaced it with a symlink to a share), which reading or canonicalizing it would
        // contact. Such a config file is forgotten instead and the editor is told it changed, so it's asked about.
        match self.load_config_file(file_path.as_ref(), None) {
          Ok(config_file) => config_file,
          Err(err) => {
            if find_config_file_on_another_machine(&err).is_some() {
              self.forget_config_file(file_path.as_ref());
              self.send_file_config_changed(file_path.as_ref(), ConfigChange::Changed);
            }
            continue;
          }
        }
      };
      let Some(config_file) = config_file else {
        // the editor isn't told when the global config file is deleted
        if previous.is_global_config
          && let Some(file_path) = previous.file_path()
        {
          self.forget_config_file(file_path.as_ref());
          if let Some(params) = self.new_found_config_changed(&previous) {
            self.send_config_changed(ConfigChangedParams {
              change: Some(ConfigChange::Deleted),
              ..params
            });
          }
        }
        continue;
      };
      let target = match config_file.file_path() {
        Some(path) => ApprovalTarget::ConfigFile {
          file_path: path.to_path_buf(),
          dir_path: config_file.is_global_config.then(|| config_file.base_path.as_ref().to_path_buf()),
        },
        // a file in its directory has it be found
        None => ApprovalTarget::File {
          file_path: config_file.base_path.as_ref().join("file"),
          use_global_config: config_file.is_global_config,
        },
      };
      self.get_approvals_for_target(&target).await;
    }
  }

  /// Writes the session file whenever what the session decided about process
  /// plugins changes (ex. a plugin the user allowed was set up, or a denial
  /// stored elsewhere replaced an allow), and again when it needs to be so
  /// that it isn't stale while the session lasts or after a write failed (see
  /// `SessionFile::needs_refresh`). Until the server holds the session's
  /// lock, it tries to take it every `SESSION_LOCK_RETRY_INTERVAL`, which
  /// writes the file once it does. It stops once the server shuts down.
  fn keep_session_file(self: &Rc<Self>) {
    let workspace = Rc::downgrade(self);
    let session_permissions = self.session_permissions.clone();
    let stopped = self.stopped.clone();
    dprint_core::async_runtime::spawn(async move {
      // the checks aren't delayed by the changes, and one that was missed (ex. while the machine was asleep) is done once
      let mut checks = tokio::time::interval_at(tokio::time::Instant::now() + SESSION_FILE_CHECK_INTERVAL, SESSION_FILE_CHECK_INTERVAL);
      checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
      loop {
        let Some(is_locked) = workspace
          .upgrade()
          .map(|workspace| workspace.session_file.borrow().as_ref().is_some_and(|file| file.is_locked()))
        else {
          return;
        };
        let is_check = tokio::select! {
          _ = stopped.cancelled() => return,
          _ = session_permissions.changed() => false,
          _ = checks.tick() => true,
          // writing tries to take the lock
          _ = tokio::time::sleep(SESSION_LOCK_RETRY_INTERVAL), if !is_locked => false,
        };
        let Some(workspace) = workspace.upgrade() else {
          return;
        };
        let session_file = workspace.session_file.borrow().clone();
        let Some(session_file) = session_file else {
          return;
        };
        if is_check && !session_file.needs_refresh(&workspace.environment) {
          continue;
        }
        match session_file.write(&workspace.environment, &workspace.get_session_state(), is_check) {
          Ok(()) => {}
          // it was logged when the session started
          Err(SessionFileWriteError::NotKept { .. }) => {}
          Err(err) => log_warn!(workspace.environment, "{}", err),
        }
      }
    });
  }

  /// Writes what the user decided for the session to the session file when
  /// it changed and there's one, which errors when it may not be what a
  /// server that's started again for the session uses (see
  /// `SessionFile::write`).
  fn save_session(&self) -> Result<(), SessionFileWriteError> {
    let session_file = self.session_file.borrow().clone();
    match session_file {
      Some(session_file) => session_file.write(&self.environment, &self.get_session_state(), false),
      None => Ok(()),
    }
  }

  /// Gets what the user decided for the session, which is what's kept in the
  /// session file. It's sorted so the same decisions are the same text.
  fn get_session_state(&self) -> SessionState {
    let allowed_items = self.allowed_download_items.borrow();
    let mut allowed_downloads = self.environment.download_policy().allowed_downloads();
    allowed_downloads.sort();
    let mut denied_downloads = self.denied_downloads.borrow().values().cloned().collect::<Vec<_>>();
    denied_downloads.sort_by(|a, b| a.id.cmp(&b.id));
    SessionState {
      permissions: self.session_permissions.state(),
      allowed_downloads: allowed_downloads
        .into_iter()
        .map(|download| AllowedDownload {
          item: allowed_items.get(&download).cloned(),
          download,
        })
        .collect(),
      denied_downloads,
    }
  }

  /// Uses what the user decided for the session in a server that stopped.
  fn restore_session_state(&self, state: SessionState) {
    self.session_permissions.restore(&self.environment, state.permissions);
    // the texts are made again because another version of dprint may escape a path another way (see `normalize_download`)
    let downloads = state
      .allowed_downloads
      .iter()
      .map(|allowed| normalize_download(&allowed.download))
      .collect::<Vec<_>>();
    self.environment.download_policy().allow(&downloads);
    self.allowed_download_items.borrow_mut().extend(
      state
        .allowed_downloads
        .into_iter()
        .filter_map(|allowed| Some((normalize_download(&allowed.download), allowed.item?))),
    );
    // and so are the ids
    self.denied_downloads.borrow_mut().extend(state.denied_downloads.into_iter().map(|denied| {
      let downloads = denied.downloads.iter().map(|download| normalize_download(download)).collect::<Vec<_>>();
      let id = get_approval_id("download", &denied.config_key, &downloads);
      (id.clone(), DeniedDownload { id, downloads, ..denied })
    }));
    // a denial of what's allowed exactly doesn't do anything (ex. kept by another version of dprint)
    let allowed = downloads.into_iter().collect::<HashSet<_>>();
    self
      .denied_downloads
      .borrow_mut()
      .retain(|_, denied| !denied.downloads.iter().all(|download| allowed.contains(download)));
    self.denied_downloads_changed();
  }

  /// Forgets a config file that was deleted by the path that identifies it,
  /// which isn't resolved again.
  fn forget_config_file(&self, file_path: &Path) {
    let is_config_file = |key: &String| key.split('\n').next().is_some_and(|source| Path::new(source) == file_path);
    // a task that resolved it before doesn't publish its problems (see `check_config_diagnostics`)
    self.checked_scopes.borrow_mut().retain(|key, _| !is_config_file(key));
    self.used_configs.borrow_mut().retain(|key, _| !is_config_file(key));
    self.resolved_configs.borrow_mut().retain(|key, _| !is_config_file(key));
    self.failed_configs.borrow_mut().retain(|key| !is_config_file(key));
    self.config_errors.borrow_mut().retain(|key, _| !is_config_file(key));
    if let Some(uri) = file_path_to_uri(file_path) {
      self.config_dependencies.borrow_mut().remove(uri.as_str());
    }
    self.register_watch_files();
  }

  /// Gets the path of the global config file when there's one.
  fn get_global_config_path(&self) -> Option<PathBuf> {
    let config_file = resolve_global_config_path_and_text(&self.environment).ok()??;
    config_file.source.maybe_local_path().map(|path| path.as_ref().to_path_buf())
  }

  /// Loads the config file at the path, which is the global config file for
  /// the root of the directory when it's the global config file (the current
  /// directory when `None`).
  ///
  /// IMPORTANT: The path is from the editor (ex. the `configFile` of an
  /// approval for reading one on another machine), so it errors with a
  /// `ConfigFileOnAnotherMachineError` before reading it when the user needs
  /// to allow reading it first, like when it's found for a file. The global
  /// config file is the user's own.
  fn load_config_file(&self, file_path: &Path, dir_path: Option<&Path>) -> Result<Option<ResolvedConfigPathWithText>> {
    if self.get_global_config_path().as_deref() != Some(file_path) {
      ensure_allows_reading_config_file(&self.environment, file_path)?;
    }
    let config_file = self.scope_container.load_config_file(file_path)?;
    match config_file {
      Some(config_file) if config_file.is_global_config => {
        let dir_path = match dir_path {
          Some(dir_path) => dir_path.to_path_buf(),
          None => self.environment.cwd().into_path_buf(),
        };
        Ok(resolve_global_config_path_and_text_at_path_root(&self.environment, &dir_path)?)
      }
      config_file => Ok(config_file),
    }
  }

  /// Shows the user the error resolving the config file, which is only done
  /// once until it resolves because requests are made without the user doing
  /// anything (ex. when a file is opened).
  fn show_config_error(&self, key: String, err: &anyhow::Error) {
    let message = format!("{:#}", err);
    let previous_message = self.config_errors.borrow_mut().insert(key, message.clone());
    if previous_message.as_ref() != Some(&message) {
      self.show_error(message);
    }
  }

  /// Shows the user the error finding a config file for the directory, which
  /// is only done when no other directory has the error.
  fn show_config_find_error(&self, dir_path: &Path, err: &anyhow::Error) {
    let message = format!("{:#}", err);
    let mut config_find_errors = self.config_find_errors.borrow_mut();
    let was_shown = config_find_errors.values().any(|shown_message| *shown_message == message);
    config_find_errors.insert(dir_path.to_path_buf(), message.clone());
    drop(config_find_errors);
    if !was_shown {
      self.show_error(message);
    }
  }

  /// Shows a language client's user an error that's in the result of a
  /// request. An editor extension shows it from the result instead because it
  /// knows whether the user made the request (ex. a format on save) or the
  /// editor did (ex. when a project is opened).
  fn show_error(&self, message: String) {
    if self.client_kind.get() == ClientKind::EditorExtension {
      log_debug!(self.environment, "{}", message);
    } else {
      self.client.show_message(MessageType::ERROR, message);
    }
  }

  /// Gets whether the global config file is used for a document when there's
  /// no config file in an ancestor directory.
  fn resolve_global_config_use(&self, global_config: Option<GlobalConfigUse>) -> bool {
    GlobalConfigUse::resolve(global_config, self.settings.borrow().use_global_config)
  }

  fn get_config_file_info(&self, scope: &PluginsScope<TEnvironment>) -> Option<ConfigFileInfo> {
    let config = scope.config.as_ref()?;
    self.new_config_file_info(ConfigFileRef::of_config(config))
  }

  /// Gets the information about a config file, which is known by the path it
  /// was found at and its directory is the one it's used for, rather than its
  /// canonical path's (ex. a `dprint.json` that's a symlink to a file in
  /// another directory).
  fn new_config_file_info(&self, config_file: ConfigFileRef<'_>) -> Option<ConfigFileInfo> {
    let is_global = config_file.is_global;
    Some(ConfigFileInfo {
      uri: self.editor_config_file_uri(config_file)?,
      dir_uri: self.editor_uri(&config_file.dir_path(&self.environment)?)?,
      kind: self.get_config_file_kind(is_global),
      in_workspace: !is_global && config_file.file_path.is_some_and(|path| self.is_in_workspace(path)),
      // IMPORTANT: It's been read, but it may be on another machine since (ex. a `git pull` replaced it with a
      // symlink to a share while a format was in progress), which is checked without following it.
      on_another_machine: !is_global && config_file.file_path.is_some_and(|path| !self.allows_reading_path(path)),
    })
  }

  /// Gets the uri of a canonicalized path as the editor knows it (ex. through
  /// a symlink), which is what the server provides.
  fn editor_uri(&self, path: &Path) -> Option<Uri> {
    file_path_to_uri(&self.editor_path(path))
  }

  /// Gets a canonicalized path as the editor knows it (see `EditorPaths`).
  ///
  /// IMPORTANT: The alias it's through is checked again first, once for each
  /// task that provides paths (see `check_editor_dirs_again`), because it
  /// may lead somewhere else now (ex. a `git pull` pointed a symlink in the
  /// workspace at a share), which the editor doesn't report, and it's
  /// forgotten then. Otherwise the editor would be given a path that it
  /// follows to another machine (ex. a file it opens) or that names another
  /// file than the one dprint read.
  fn editor_path(&self, path: &Path) -> PathBuf {
    while let Some((canonical_dir, editor_dir)) = self.editor_paths.get_alias(path) {
      if self.check_editor_dir(&canonical_dir, &editor_dir) {
        break;
      }
    }
    self.editor_paths.get(path)
  }

  /// Checks that the editor's path of the canonical directory is still an
  /// alias of it (see `editor_path`), which is only done once for each task.
  /// It's forgotten when it isn't.
  fn check_editor_dir(&self, canonical_dir: &Path, editor_dir: &Path) -> bool {
    if self.checked_editor_dirs.borrow().contains(canonical_dir) {
      return true;
    }
    // IMPORTANT: It's checked before it's canonicalized, relative to the directory it was an alias of, so one that
    // leads to the share of that directory is still an alias (ex. a workspace folder that's a symlink to a share),
    // while one that leads to another machine isn't, and isn't followed.
    let is_alias = self
      .environment
      .download_policy()
      .ensure_allows_path(&self.environment, editor_dir, Some(canonical_dir))
      .is_ok()
      && self
        .environment
        .canonicalize_maybe_not_exists(editor_dir)
        .is_ok_and(|dir| dir.as_ref() == canonical_dir);
    if is_alias {
      self.checked_editor_dirs.borrow_mut().insert(canonical_dir.to_path_buf());
    } else {
      self.editor_paths.remove(canonical_dir);
      self.take_back_diagnostics_under(editor_dir);
    }
    is_alias
  }

  /// Has the aliases of `editor_paths` checked again before they're used next
  /// (see `editor_path`), which is done at the start of each task of the
  /// language server (ex. a request) and of each one of the workspace's own
  /// that provides paths, so they're checked once for it.
  ///
  /// The ones that problems were published through are checked now, because
  /// the editor may only use paths through them, which don't lead to them
  /// (see `take_back_diagnostics_under`).
  pub fn check_editor_dirs_again(&self) {
    self.checked_editor_dirs.borrow_mut().clear();
    let editor_paths = self
      .published_diagnostics
      .borrow()
      .values()
      .filter_map(|(editor_uri, _)| uri_to_file_path(editor_uri))
      .collect::<Vec<_>>();
    for editor_path in editor_paths {
      if let Some((canonical_dir, editor_dir)) = self.editor_paths.get_alias_of_editor_path(&editor_path) {
        self.check_editor_dir(&canonical_dir, &editor_dir);
      }
    }
  }

  /// Gets the uri of a config file as the editor knows it.
  fn editor_config_file_uri(&self, config_file: ConfigFileRef<'_>) -> Option<Uri> {
    match config_file.file_path {
      Some(path) => self.editor_uri(path),
      None => path_source_to_uri(config_file.source),
    }
  }

  /// Gets the message that asks whether to download what isn't trusted (see
  /// `get_untrusted_download_message`) with the editor's path of each `.npmrc`
  /// file.
  fn get_untrusted_download_message(&self, downloads: &[String]) -> String {
    let downloads = downloads
      .iter()
      .map(|download| match NpmrcProxy::from_download(download) {
        Some(proxy) => NpmrcProxy {
          npmrc_path: self.editor_path(Path::new(&proxy.npmrc_path)).display().to_string(),
          ..proxy
        }
        .to_download(),
        None => download.clone(),
      })
      .collect::<Vec<_>>();
    get_untrusted_download_message(&downloads, read_ignored_settings_message(&self.environment).as_deref())
  }

  /// Gets the notification that a local config file the editor said changed did.
  fn new_file_config_changed(&self, file_path: &Path, change: ConfigChange) -> Option<ConfigChangedParams> {
    Some(new_identified_config_changed(
      self.editor_uri(file_path)?,
      self.editor_uri(get_config_file_dir(&self.environment, file_path)?)?,
      change,
    ))
  }

  /// Gets the notification that the config changed with the directory it's used for.
  fn new_config_changed(&self, config: &ResolvedConfig, change: ConfigChange) -> Option<ConfigChangedParams> {
    let config_file = ConfigFileRef::of_config(config);
    Some(new_identified_config_changed(
      self.editor_config_file_uri(config_file)?,
      self.editor_uri(&config_file.dir_path(&self.environment)?)?,
      change,
    ))
  }

  /// Gets the notification that a config file that was found changed.
  fn new_found_config_changed(&self, config_file: &ResolvedConfigPathWithText) -> Option<ConfigChangedParams> {
    let config_file = ConfigFileRef::of_found(config_file);
    Some(new_identified_config_changed(
      self.editor_config_file_uri(config_file)?,
      self.editor_uri(&config_file.dir_path(&self.environment)?)?,
      ConfigChange::Changed,
    ))
  }

  /// Gets whether the canonicalized path is in a workspace folder.
  fn is_in_workspace(&self, file_path: &Path) -> bool {
    self.canonical_workspace_folders.borrow().iter().any(|folder| file_path.starts_with(folder))
  }

  fn get_config_file_kind(&self, is_global: bool) -> ConfigFileKind {
    if self.scope_container.has_config_override() {
      ConfigFileKind::Cli
    } else if is_global {
      ConfigFileKind::Global
    } else {
      ConfigFileKind::Local
    }
  }

  /// Gets why there's no config file for a file that doesn't have one in an
  /// ancestor directory.
  fn get_no_config_reason(&self, use_global_config: bool) -> NotFormattedReason {
    if !use_global_config && matches!(resolve_global_config_path_and_text(&self.environment), Ok(Some(_))) {
      NotFormattedReason::GlobalConfigNotAllowed
    } else {
      NotFormattedReason::NoConfigFile
    }
  }

  /// Gets why the scope doesn't format a file. It's left to the editor's other
  /// formatters when something needs to be approved and it's not known
  /// whether the config file formats it.
  fn new_scope_not_formatted(
    &self,
    scope: &PluginsScope<TEnvironment>,
    reason: NotFormattedReason,
    is_notebook_cell: bool,
    use_global_config: bool,
    approvals: Vec<PendingApproval>,
  ) -> NotFormatted {
    // what needs to be approved doesn't matter for a file it doesn't format
    let approvals = match reason {
      NotFormattedReason::ApprovalRequired => approvals,
      _ => Vec::new(),
    };
    let not_formatted = NotFormatted::new(reason, self.get_reason_message(scope, reason, is_notebook_cell, use_global_config), approvals);
    let are_plugins_known = scope.config.as_ref().is_some_and(|config| self.get_last_plugin_infos(config).is_some());
    if reason == NotFormattedReason::ApprovalRequired && !are_plugins_known {
      not_formatted.unclaimed()
    } else {
      not_formatted
    }
  }

  /// Gets why a config file that failed to resolve doesn't format a file.
  ///
  /// A file is only claimed when the plugins the config file had the last
  /// time it was used may format it, so that an error (ex. a typo) doesn't
  /// change what the editor formats with it, and none are when they aren't
  /// known (ex. it had the error since the server started).
  fn get_failure_not_formatted(&self, failure: &ConfigFailure, target: &ApprovalTarget) -> NotFormatted {
    let actions = match (&failure.config_file, failure.reason) {
      (Some(config_file), NotFormattedReason::ConfigError) => vec![open_config_file_action(&config_file.uri)],
      _ => Vec::new(),
    };
    let not_formatted = NotFormatted::new(failure.reason, Some(failure.message.clone()), self.get_failure_approvals(failure, target)).with_actions(actions);
    let is_claimed = match (failure.reason, target) {
      // what it formats isn't known until what it extends is downloaded
      (NotFormattedReason::ApprovalRequired, _) => false,
      (_, ApprovalTarget::File { file_path, .. }) => {
        let last_plugin_infos = self.last_plugin_infos.borrow();
        let last = failure.config_key.as_ref().and_then(|(key, _)| last_plugin_infos.get(key));
        last.is_some_and(|last| may_plugins_format(&last.config, &last.plugins, file_path))
      }
      // the status of the config file
      (_, ApprovalTarget::ConfigFile { .. }) => true,
    };
    if is_claimed { not_formatted } else { not_formatted.unclaimed() }
  }

  /// Gets why there's no config file for a target.
  fn get_not_found_not_formatted(&self, target: &ApprovalTarget) -> NotFormatted {
    let (reason, message) = match target {
      ApprovalTarget::File { use_global_config, .. } => {
        let reason = self.get_no_config_reason(*use_global_config);
        (reason, self.get_not_formatted_message(reason, false, *use_global_config))
      }
      ApprovalTarget::ConfigFile { file_path, .. } => (
        NotFormattedReason::ConfigFileMissing,
        Some(format!("The configuration file doesn't exist: {}", file_path.display())),
      ),
    };
    NotFormatted::new(reason, message, Vec::new())
  }

  /// Gets the message for why the scope doesn't format a file.
  fn get_reason_message(
    &self,
    scope: &PluginsScope<TEnvironment>,
    reason: NotFormattedReason,
    is_notebook_cell: bool,
    use_global_config: bool,
  ) -> Option<String> {
    match (reason, &scope.unapproved_process_plugins) {
      (NotFormattedReason::ApprovalRequired, _) if !scope.untrusted_downloads.is_empty() => {
        Some(self.get_untrusted_download_message(&scope.untrusted_downloads))
      }
      (NotFormattedReason::ApprovalRequired, Some(unapproved)) => Some(unapproved.message(&self.session_permissions.read(&self.environment))),
      _ => self.get_not_formatted_message(reason, is_notebook_cell, use_global_config),
    }
  }

  /// Gets the message to show the user about why a file wasn't formatted (ex.
  /// for a command the user ran).
  fn get_not_formatted_message(&self, reason: NotFormattedReason, is_notebook_cell: bool, use_global_config: bool) -> Option<String> {
    match reason {
      NotFormattedReason::NoConfigFile => Some(get_no_config_message(NoConfigMessageOptions {
        use_global_config,
        has_global_config: false,
        settings_owner: self.get_settings_owner(),
      })),
      NotFormattedReason::GlobalConfigNotAllowed => Some(get_no_config_message(NoConfigMessageOptions {
        use_global_config: false,
        has_global_config: true,
        settings_owner: self.get_settings_owner(),
      })),
      NotFormattedReason::NotMatched => Some(NOT_MATCHED_MESSAGE.to_string()),
      NotFormattedReason::NoPlugin if is_notebook_cell => Some(NO_CELL_PLUGIN_MESSAGE.to_string()),
      NotFormattedReason::NoPlugin => Some(NO_PLUGIN_MESSAGE.to_string()),
      // the message is created with the diagnostics instead
      NotFormattedReason::ConfigDiagnostics => None,
      // the message is the error
      NotFormattedReason::ConfigError => None,
      NotFormattedReason::RangeFormattingNotSupported => Some(RANGE_FORMATTING_NOT_SUPPORTED_MESSAGE.to_string()),
      // the message is the approval's
      NotFormattedReason::ApprovalRequired => None,
      // the message is the error's
      NotFormattedReason::UnsupportedDocument | NotFormattedReason::LanguageRequired | NotFormattedReason::ConfigFileMissing => None,
    }
  }

  /// Gets whose settings to mention to the user (ex. to use the global config file).
  fn get_settings_owner(&self) -> &'static str {
    match self.client_kind.get() {
      ClientKind::LanguageClient { .. } => LANGUAGE_SERVER_SETTINGS_OWNER,
      ClientKind::EditorExtension => EDITOR_EXTENSION_SETTINGS_OWNER,
    }
  }

  /// Gets why the scope can't format the file, which is `None` when it can.
  fn get_cant_format_reason(
    &self,
    scope: &PluginsScope<TEnvironment>,
    file_path: &Path,
    notebook_path: Option<&Path>,
    file_bytes: Option<&[u8]>,
  ) -> Option<NotFormattedReason> {
    if !scope.untrusted_downloads.is_empty() || scope.unapproved_process_plugins.is_some() {
      return Some(self.get_approval_required_reason(scope, notebook_path.unwrap_or(file_path)));
    }
    match notebook_path {
      // the cli formats a notebook's cells when a plugin (the jupyter plugin) formats
      // the notebook, so only format a cell when the notebook would be formatted
      Some(notebook_path) => {
        if !scope.can_format_for_editor(notebook_path, None) {
          return Some(NotFormattedReason::NotMatched);
        }
        if scope.plugin_name_maps.get_plugin_names_from_file_path(notebook_path).into_names().is_empty() {
          return Some(NotFormattedReason::NoPlugin);
        }
      }
      None => {
        if !scope.can_format_for_editor(file_path, file_bytes) {
          return Some(NotFormattedReason::NotMatched);
        }
      }
    }
    let has_plugin = match file_bytes {
      Some(file_bytes) => !scope
        .plugin_name_maps
        .get_plugin_names_from_file_path_and_bytes(file_path, file_bytes)
        .is_empty(),
      None => !get_plugin_names_for_file_on_disk(&scope.plugin_name_maps, file_path, &self.environment).is_empty(),
    };
    if has_plugin { None } else { Some(NotFormattedReason::NoPlugin) }
  }

  /// Gets why a scope whose plugins need to be approved doesn't format a file,
  /// which says it isn't matched when its plugins didn't match it the last
  /// time they were known so that the editor doesn't ask about approving
  /// them for a file they wouldn't format (ex. a readme).
  fn get_approval_required_reason(&self, scope: &PluginsScope<TEnvironment>, file_path: &Path) -> NotFormattedReason {
    if !scope.matches_editor_file_patterns(file_path) {
      return NotFormattedReason::NotMatched;
    }
    let last_plugins = scope.config.as_ref().and_then(|config| Some((config, self.get_last_plugin_infos(config)?)));
    match last_plugins {
      Some((config, plugins)) if !may_plugins_format(config, &plugins, file_path) => NotFormattedReason::NoPlugin,
      _ => NotFormattedReason::ApprovalRequired,
    }
  }

  /// Gets the files the document may be formatted as.
  fn get_document_file(&self, document: &DocumentInfo) -> Result<DocumentFile> {
    // the document is formatted as the file instead (ex. a document with another scheme)
    if let Some(file_uri) = &document.file_uri {
      let Some(file_path) = uri_to_file_path(file_uri).filter(|path| path.parent().is_some()) else {
        return Err(UnsupportedDocumentError::unsupported(format!(
          "Cannot format a document as a file that isn't a file path: {}",
          file_uri.as_str()
        )));
      };
      return Ok(DocumentFile {
        candidates: vec![file_path],
        is_document_path: false,
        notebook_path: None,
      });
    }
    if let Some(notebook_uri) = &document.notebook_uri {
      // the cli only formats notebooks on the file system
      let Some(notebook_path) = uri_to_file_path(notebook_uri) else {
        return Err(UnsupportedDocumentError::unsupported(format!(
          "Cannot format cell of a notebook that is not on the file system: {}",
          notebook_uri.as_str()
        )));
      };
      let Some(dir_path) = notebook_path.parent() else {
        return Err(UnsupportedDocumentError::unsupported(format!(
          "Cannot format cell of a notebook without a directory: {}",
          notebook_path.display()
        )));
      };
      return Ok(DocumentFile {
        candidates: get_language_file_paths(dir_path, "code_block", document)?,
        is_document_path: false,
        notebook_path: Some(notebook_path),
      });
    }
    if is_untitled_uri(&document.uri) {
      return Ok(DocumentFile {
        candidates: get_language_file_paths(&self.get_untitled_dir_path(&document.uri)?, "Untitled", document)?,
        is_document_path: false,
        notebook_path: None,
      });
    }
    let file_path = self.get_uri_file_path(&document.uri)?;
    if file_path.parent().is_none() {
      return Err(UnsupportedDocumentError::unsupported(format!(
        "Cannot format non-file path: {}",
        file_path.display()
      )));
    }
    Ok(DocumentFile {
      candidates: vec![file_path],
      is_document_path: true,
      notebook_path: None,
    })
  }

  /// Gets the path of a file for the uri, which is in the directory an
  /// untitled document is formatted in for an untitled uri.
  fn get_uri_file_path(&self, uri: &Uri) -> Result<PathBuf> {
    if is_untitled_uri(uri) {
      return Ok(self.get_untitled_dir_path(uri)?.join("Untitled"));
    }
    match uri_to_file_path(uri) {
      Some(file_path) => Ok(file_path),
      None => Err(UnsupportedDocumentError::unsupported(format!(
        "Cannot format document that is not a file, an untitled document or a notebook cell: {}",
        uri.as_str()
      ))),
    }
  }

  /// Gets the directory that an untitled document is formatted in, which is
  /// the first workspace folder or otherwise the home directory.
  fn get_untitled_dir_path(&self, uri: &Uri) -> Result<PathBuf> {
    let workspace_folder = self.settings.borrow().workspace_folders.first().cloned();
    match workspace_folder.or_else(|| self.environment.get_home_dir().map(|dir| dir.into_path_buf())) {
      Some(dir_path) => Ok(dir_path),
      None => Err(UnsupportedDocumentError::unsupported(format!(
        "Cannot format untitled document without a workspace folder or home directory: {}",
        uri.as_str()
      ))),
    }
  }

  /// Chooses the file to format the document as, which is the first one the
  /// scope can format or otherwise the first one.
  fn choose_file(&self, scope: &PluginsScope<TEnvironment>, document_file: DocumentFile, file_bytes: Option<&[u8]>) -> ChosenFile {
    // the editor opened the document and the notebook, which reading them doesn't change
    let notebook_path = document_file.notebook_path.map(|path| self.canonicalize(path));
    let mut first = None;
    for candidate in document_file.candidates {
      let file_path = match document_file.is_document_path {
        true => self.canonicalize(candidate),
        // ex. the file an untitled document is formatted as may be a symlink in the workspace to another machine
        false => self.canonicalize_editor_path(candidate),
      };
      let cant_format_reason = self.get_cant_format_reason(scope, &file_path, notebook_path.as_deref(), file_bytes);
      if cant_format_reason.is_none() {
        return ChosenFile { file_path, cant_format_reason };
      }
      first.get_or_insert((file_path, cant_format_reason));
    }
    let (file_path, cant_format_reason) = first.expect("there should be a candidate");
    ChosenFile { file_path, cant_format_reason }
  }

  /// Canonicalizes the path so that it matches the canonicalized config paths
  /// (ex. for the includes and excludes).
  ///
  /// IMPORTANT: It follows symlinks, which contacts the machine a path is on,
  /// so a path from the editor that wasn't checked yet is canonicalized with
  /// `canonicalize_editor_path` instead.
  fn canonicalize(&self, path: PathBuf) -> PathBuf {
    let canonical_path = self
      .environment
      .canonicalize_maybe_not_exists(&path)
      .map(|p| p.into_path_buf())
      .unwrap_or(path.clone());
    self.remember_editor_path(&path, &canonical_path);
    canonical_path
  }

  /// Canonicalizes a path from the editor (ex. of a file it says changed)
  /// like `canonicalize` when reading it is allowed. When it's on another
  /// machine and reading it isn't allowed yet (ex. a `git pull` replaced a
  /// config file with a symlink to a share), only its directory is
  /// canonicalized when that's allowed, so it's matched without following it,
  /// and otherwise it's kept as it is.
  ///
  /// IMPORTANT: Canonicalizing a path follows its symlinks, which contacts
  /// the other machine, so it's only done once reading it is allowed (see
  /// `DownloadPolicy::ensure_allows_path`), like reading a config file.
  fn canonicalize_editor_path(&self, path: PathBuf) -> PathBuf {
    if self.allows_reading_path(&path) {
      return self.canonicalize(path);
    }
    match (path.parent(), path.file_name()) {
      (Some(dir_path), Some(file_name)) if self.allows_reading_path(dir_path) => {
        let canonical_path = match self.environment.canonicalize_maybe_not_exists(dir_path) {
          Ok(canonical_dir_path) => canonical_dir_path.into_path_buf().join(file_name),
          Err(_) => path.clone(),
        };
        self.remember_editor_path(&path, &canonical_path);
        canonical_path
      }
      _ => path,
    }
  }

  /// Canonicalizes the path of a local config file from the editor to the path
  /// that identifies it like `canonicalize_editor_path`, which is its path in
  /// the directory it's used for when it's a symlink to another file (see
  /// `get_config_file_found_path`).
  fn canonicalize_config_file_path(&self, path: PathBuf) -> PathBuf {
    let canonical_path = self.canonicalize_editor_path(path.clone());
    self.get_config_file_identity_path(&path, canonical_path)
  }

  /// Gets the path that identifies a local config file at the editor's path
  /// with its canonical path (see `canonicalize_config_file_path`).
  fn get_config_file_identity_path(&self, path: &Path, canonical_path: PathBuf) -> PathBuf {
    let Some(lookup) = get_config_file_lookup(&self.environment, path) else {
      return canonical_path;
    };
    // a directory that may not be read had only that canonicalized, which is the path then
    if !self.allows_reading_path(lookup.base_dir) {
      return canonical_path;
    }
    match self.environment.canonicalize_maybe_not_exists(lookup.base_dir) {
      Ok(canonical_dir_path) => get_config_file_found_path(canonical_dir_path.as_ref().join(lookup.relative_path()), &canonical_path).unwrap_or(canonical_path),
      Err(_) => canonical_path,
    }
  }

  /// Gets whether each download is allowed exactly (see `TrustedDownloads::is_allowed`).
  fn is_each_allowed(&self, downloads: &[String]) -> bool {
    let allowed = self.environment.download_policy().allowed_downloads().into_iter().collect::<HashSet<_>>();
    downloads.iter().all(|download| allowed.contains(download))
  }

  /// Gets whether a path from the editor may be read without asking the user
  /// (see `DownloadPolicy::ensure_allows_path`), which is relative to its
  /// directory like a config file is (see `ensure_allows_reading_config_file`).
  fn allows_reading_path(&self, path: &Path) -> bool {
    self
      .environment
      .download_policy()
      .ensure_allows_path(&self.environment, path, path.parent())
      .is_ok()
  }

  /// Remembers that the editor's path is an alias of the canonical path,
  /// because the paths the server provides are the editor's (ex. a workspace
  /// opened through a symlink).
  fn remember_editor_path(&self, path: &Path, canonical_path: &Path) {
    if canonical_path != path && self.editor_paths.get(canonical_path) != path {
      self.add_editor_dir(path, canonical_path);
    }
  }

  /// Remembers the directory of the editor's path that's an alias of the
  /// directory of the canonical path, which is the highest one where they're
  /// the same below it. The file itself isn't one (ex. a symlink to a file in
  /// another directory doesn't make its directory an alias).
  fn add_editor_dir(&self, editor_path: &Path, canonical_path: &Path) {
    let mut alias = None;
    let mut dirs = (editor_path.parent(), canonical_path.parent());
    while let (Some(editor_dir), Some(canonical_dir)) = dirs {
      if editor_dir == canonical_dir {
        break;
      }
      let is_alias = self
        .environment
        .canonicalize_maybe_not_exists(editor_dir)
        .is_ok_and(|dir| dir.as_ref() == canonical_dir);
      if is_alias {
        alias = Some((editor_dir, canonical_dir));
      }
      // the directories above it are only the same when it's the same directory
      if editor_dir.file_name() != canonical_dir.file_name() {
        break;
      }
      dirs = (editor_dir.parent(), canonical_dir.parent());
    }
    if let Some((editor_dir, canonical_dir)) = alias {
      // an editor's path that was an alias of another directory names another file now
      for replaced_editor_dir in self.editor_paths.add(editor_dir, canonical_dir) {
        self.take_back_diagnostics_under(&replaced_editor_dir);
      }
      // it was found to be an alias by following it, which only happens when that's allowed
      self.checked_editor_dirs.borrow_mut().insert(canonical_dir.to_path_buf());
    }
  }
}

/// Gets the information about the plugins of the scope, which sets them up
/// when they aren't yet (ex. downloads them).
async fn get_plugin_infos<TEnvironment: Environment>(scope: &PluginsScope<TEnvironment>) -> Result<Vec<PluginInfo>> {
  let mut plugins = Vec::new();
  for plugin in scope.plugins.values() {
    let file_matching = plugin.initialize().await?.file_matching_info().await?;
    plugins.push(get_plugin_info(plugin.info().clone(), file_matching));
  }
  Ok(plugins)
}

fn get_plugin_info(info: dprint_core::plugins::PluginInfo, file_matching: FileMatchingInfo) -> PluginInfo {
  PluginInfo {
    name: info.name,
    version: info.version,
    config_key: info.config_key,
    file_extensions: file_matching.file_extensions,
    file_names: file_matching.file_names,
    additive: file_matching.additive,
    range_formatting: match info.supports_range_formatting {
      Some(true) => RangeFormatting::Supported,
      Some(false) => RangeFormatting::Unsupported,
      None => RangeFormatting::WholeLines,
    },
    help_url: info.help_url,
    update_url: info.update_url,
  }
}

fn get_file_plugin(info: &dprint_core::plugins::PluginInfo) -> FilePlugin {
  FilePlugin {
    name: info.name.clone(),
    version: info.version.clone(),
    config_key: info.config_key.clone(),
  }
}

/// Gets the globs of the files the plugins may format, which are relative to
/// the directory of the config file. A file extension or name matches case
/// insensitively like dprint does.
fn get_file_patterns(config: &ResolvedConfig, plugins: &[PluginInfo]) -> (Vec<FilePattern>, Vec<OtherFiles>) {
  let mut patterns: Vec<FilePattern> = Vec::new();
  let mut other_associations = Vec::new();
  let mut add = |glob: String, config_key: &str| match patterns.iter_mut().find(|pattern| pattern.glob == glob) {
    Some(pattern) => {
      if !pattern.config_keys.iter().any(|key| key == config_key) {
        pattern.config_keys.push(config_key.to_string());
      }
    }
    None => patterns.push(FilePattern {
      glob,
      config_keys: vec![config_key.to_string()],
    }),
  };
  for plugin in plugins {
    for extension in &plugin.file_extensions {
      add(format!("**/*.{}", to_case_insensitive_glob(extension)), &plugin.config_key);
    }
    for file_name in &plugin.file_names {
      add(format!("**/{}", to_case_insensitive_glob(file_name)), &plugin.config_key);
    }
    if let Some(ConfigMapValue::PluginConfig(plugin_config)) = config.config_map.get(&plugin.config_key) {
      for association in plugin_config.associations.iter().flatten() {
        // the associations are matched like the includes and excludes, so they're made relative to the directory
        let globs = to_dir_relative_globs(association, config.pattern_bases(), config.base_path.as_ref());
        if globs.is_empty() && !association.starts_with('!') {
          // ex. one in an ancestor directory
          other_associations.push(association.clone());
        }
        for glob in globs {
          add(glob, &plugin.config_key);
        }
      }
    }
  }
  let mut other_files = Vec::new();
  if let Some(shebangs) = config.shebangs.as_ref().filter(|shebangs| !shebangs.is_empty()) {
    other_files.push(OtherFiles {
      kind: OtherFilesKind::Shebangs,
      values: shebangs.keys().cloned().collect(),
    });
  }
  if !other_associations.is_empty() {
    other_files.push(OtherFiles {
      kind: OtherFilesKind::Associations,
      values: other_associations,
    });
  }
  (patterns, other_files)
}

fn get_decision_options(kind: ApprovalKind, decisions: &[ApprovalDecision]) -> Vec<ApprovalDecisionOption> {
  decisions
    .iter()
    .map(|decision| ApprovalDecisionOption {
      description: match (kind, decision) {
        (ApprovalKind::Downloads, ApprovalDecision::Allow) => "Allows these downloads (and proxies) until the editor restarts.",
        (ApprovalKind::Downloads, _) => "Doesn't download, and doesn't ask again until the editor restarts unless you ask for it.",
        (ApprovalKind::ProcessPlugins, ApprovalDecision::Allow) => {
          "Runs these process plugins with this configuration for this configuration file until the editor restarts."
        }
        (ApprovalKind::ProcessPlugins, ApprovalDecision::AlwaysAllow) => {
          "Runs these process plugins with this configuration for this configuration file from now on."
        }
        (ApprovalKind::ProcessPlugins, ApprovalDecision::Deny) => "Doesn't run them, and doesn't ask again until the editor restarts unless you ask for it.",
        (ApprovalKind::ProcessPlugins, ApprovalDecision::AlwaysDeny) => "Doesn't run them, and doesn't ask again unless you ask for it.",
      }
      .to_string(),
      decision: *decision,
      label: match decision {
        ApprovalDecision::Allow => "Allow",
        ApprovalDecision::AlwaysAllow => "Always Allow",
        ApprovalDecision::Deny => "Deny",
        ApprovalDecision::AlwaysDeny => "Always Deny",
      }
      .to_string(),
      allows: decision.is_allowed(),
      always: matches!(decision, ApprovalDecision::AlwaysAllow | ApprovalDecision::AlwaysDeny),
    })
    .collect()
}

/// Gets the notification that the config file used for the directory changed.
fn new_identified_config_changed(uri: Uri, dir_uri: Uri, change: ConfigChange) -> ConfigChangedParams {
  ConfigChangedParams {
    config_file: Some(ConfigFileIdentifier { uri, dir_uri: Some(dir_uri) }),
    change: Some(change),
    schema_only: false,
  }
}

/// Gets the notification that every config file may have changed.
fn new_every_config_changed() -> ConfigChangedParams {
  ConfigChangedParams {
    config_file: None,
    change: None,
    schema_only: false,
  }
}

/// Gets what identifies the config file of the notification.
fn get_config_changed_key(params: &ConfigChangedParams) -> Option<String> {
  let config_file = params.config_file.as_ref()?;
  Some(match &config_file.dir_uri {
    Some(dir_uri) => format!("{}\n{}", config_file.uri.as_str(), dir_uri.as_str()),
    None => config_file.uri.as_str().to_string(),
  })
}

/// Gets the message to ask a language client's user about the approval,
/// which has what the process plugins run with so it can be seen before
/// deciding (an editor extension shows it itself).
fn get_language_client_approval_message(approval: &PendingApproval) -> String {
  let mut message = approval.message.clone();
  let configuration = approval
    .items
    .iter()
    .filter_map(|item| Some((item.plugin.as_ref()?.config_key.clone(), item.configuration.clone()?)))
    .collect::<serde_json::Map<_, _>>();
  if !configuration.is_empty() {
    message.push_str(&format!(
      " They run with this configuration: {}",
      get_escaped_json(&serde_json::Value::Object(configuration))
    ));
  }
  if let Some(trusted_configuration) = &approval.trusted_configuration {
    message.push_str(&format!(
      " The plugins that weren't set up before run with this configuration: {}",
      get_escaped_json(trusted_configuration)
    ));
  }
  message
}

/// Gets the JSON text of a configuration from the project that's shown in a
/// message, whose strings escape what text from the project does (see
/// `escape_text`) as `\uXXXX`, which is still the same JSON. That's what
/// `serde_json` doesn't escape (ex. U+2028 or a bidi override like U+202E,
/// which would show the rest of the message backwards), and a combining mark
/// at the start of a string, which would change its quote.
fn get_escaped_json(value: &serde_json::Value) -> String {
  let text = value.to_string();
  let mut escaped = String::with_capacity(text.len());
  let mut previous = None;
  for c in text.chars() {
    // what's outside of a string is only JSON's own characters, which aren't escaped
    if is_escaped_in_text(c) || (previous == Some('"') && is_combining_mark(c)) {
      let mut units = [0; 2];
      for unit in c.encode_utf16(&mut units).iter() {
        escaped.push_str(&format!("\\u{:04x}", unit));
      }
    } else {
      escaped.push(c);
    }
    previous = Some(c);
  }
  escaped
}

/// Gets whether a denial is for the config file as the process plugin
/// decisions have it with its key, or for every config file when `None`.
///
/// It's also for the config file the editor sent when that's the one it
/// shows (`Decision::config_file`), because another version of dprint may
/// have made its key another way (ex. it restored the denial from a session
/// file that version wrote), so what `dprint/decisions` provides can always
/// be forgotten. A key that doesn't match is otherwise kept, which only asks
/// about it again.
fn is_denial_for(denied: &DeniedDownload, config_file: Option<&(String, String)>, identifier: Option<&(Uri, Option<Uri>)>) -> bool {
  let Some((_, key)) = config_file else {
    return true;
  };
  let is_shown_config_file = identifier.is_some_and(|(uri, dir_uri)| {
    denied.config_file.uri == *uri && dir_uri.as_ref().is_none_or(|dir_uri| denied.config_file.dir_uri.as_ref() == Some(dir_uri))
  });
  is_config_key(key, &denied.config_key) || is_shown_config_file
}

/// Gets whether a config key is of the config file the key is of, which is
/// the config key of each directory the global config file is used for when
/// the key doesn't have the directory (see `get_found_config_key`).
fn is_config_key(key: &str, config_key: &str) -> bool {
  config_key == key || (!key.contains('\n') && config_key.strip_prefix(key).is_some_and(|rest| rest.starts_with('\n')))
}

/// Gets a notice that's a warning the user is always told about.
fn new_warning_notice(message: String) -> Notice {
  Notice {
    message,
    show: ShowPolicy::Always,
    severity: Severity::Warning,
    actions: Vec::new(),
  }
}

/// Adds a warning the user is always told about to the notice, which is after
/// what the notice says already when there's one.
fn push_warning_notice(notice: &mut Option<Notice>, message: String) {
  match notice {
    Some(notice) => {
      notice.message.push(' ');
      notice.message.push_str(&message);
      notice.show = ShowPolicy::Always;
      if notice.severity == Severity::Info {
        notice.severity = Severity::Warning;
      }
    }
    None => *notice = Some(new_warning_notice(message)),
  }
}

/// Gets the action that opens a config file.
fn open_config_file_action(uri: &Uri) -> Action {
  Action::OpenUri {
    label: "Open Configuration File".to_string(),
    uri: uri.clone(),
  }
}

/// Gets whether the plugins may format the file by its name, extension, or
/// the associations in the config, which is for when they can't be used (ex.
/// they need to be approved). An extensionless file may have a shebang.
fn may_plugins_format(config: &ResolvedConfig, plugins: &[PluginInfo], file_path: &Path) -> bool {
  if file_path.extension().is_none() && config.shebangs.as_ref().is_some_and(|shebangs| !shebangs.is_empty()) {
    return true;
  }
  let file_name = get_lowercase_file_name(file_path);
  let extension = get_lowercase_file_extension(file_path);
  plugins.iter().any(|plugin| {
    let matches_name = file_name
      .as_deref()
      .is_some_and(|file_name| plugin.file_names.iter().any(|name| name.to_lowercase() == file_name));
    let matches_extension = extension.as_deref().is_some_and(|extension| {
      plugin
        .file_extensions
        .iter()
        .any(|plugin_extension| plugin_extension.to_lowercase() == extension)
    });
    let matches_association = match config.config_map.get(&plugin.config_key) {
      Some(ConfigMapValue::PluginConfig(plugin_config)) => plugin_config
        .associations
        .as_deref()
        .and_then(|associations| OrderedPatternsMatcher::new(associations, config.pattern_bases()).ok())
        .is_some_and(|matcher| matcher.matches(file_path)),
      _ => false,
    };
    matches_name || matches_extension || matches_association
  })
}

/// Gets a glob that matches the text case insensitively (ex. `[tT][sS]`).
fn to_case_insensitive_glob(text: &str) -> String {
  let mut glob = String::new();
  for c in text.chars() {
    let lowercase = c.to_lowercase().collect::<String>();
    let uppercase = c.to_uppercase().collect::<String>();
    if lowercase != uppercase {
      glob.push('[');
      glob.push_str(&lowercase);
      glob.push_str(&uppercase);
      glob.push(']');
    } else if matches!(c, '*' | '?' | '[' | '{' | '}') {
      // a character the glob would otherwise treat specially, which is a class
      // of its own (`!` and `]` are only special within a class)
      glob.push('[');
      glob.push(c);
      glob.push(']');
    } else {
      glob.push(c);
    }
  }
  glob
}

/// Gets whether the file is a config file that's found for a directory by
/// its name, like dprint finds them for a file.
pub fn is_config_file_path(environment: &impl Environment, file_path: &Path) -> bool {
  // the names dprint looks up, which a case insensitive file system finds in another case too
  let Some(lookup) = get_config_file_lookup(environment, file_path) else {
    return false;
  };
  let is_looked_up_name = lookup.file_name.to_str().is_some_and(|name| POSSIBLE_CONFIG_FILE_NAMES.contains(&name));
  // only the names of `DOT_CONFIG_FILE_NAMES` are found in a `.config` directory, which isn't the home directory's
  is_looked_up_name && (lookup.is_in_dot_config_dir || !lookup.is_parent_dot_config_dir)
}

/// Gets the directory a local config file is used for, which is the
/// directory it's in unless that's a `.config` directory (other than the home
/// directory's, which is an ordinary directory).
fn get_config_file_dir<'a>(environment: &impl Environment, file_path: &'a Path) -> Option<&'a Path> {
  let dir_path = file_path.parent()?;
  Some(match is_dot_config_dir(environment, dir_path) {
    true => dir_path.parent().unwrap_or(dir_path),
    false => dir_path,
  })
}

/// Gets what identifies each plugin of the config.
fn get_plugin_references(config: &ResolvedConfig) -> Vec<String> {
  config.plugins.iter().map(|plugin| plugin.to_full_string()).collect()
}

/// Gets the key of what's approved for the config, which is for each
/// directory the global config file is used for.
fn get_config_key(config: &ResolvedConfig) -> String {
  let key = ConfigFileRef::of_config(config)
    .uri()
    .map(|uri| uri.as_str().to_string())
    .unwrap_or_else(|| config.display());
  match config.is_global {
    true => format!("{}\n{}", key, config.base_path.as_ref().display()),
    false => key,
  }
}

/// Finds the error for a config file that needs the user to allow reading it
/// from another machine (see `ensure_allows_reading_config_file`).
fn find_config_file_on_another_machine(err: &anyhow::Error) -> Option<&ConfigFileOnAnotherMachineError> {
  err.chain().find_map(|err| err.downcast_ref::<ConfigFileOnAnotherMachineError>())
}

/// Gets the key of a config file that was found (see `get_config_key`) with
/// the uri that identifies it (see `ResolvedConfig::file_path`).
fn get_found_config_key(config_file: &ResolvedConfigPathWithText) -> Option<(String, Uri)> {
  let uri = ConfigFileRef::of_found(config_file).uri()?;
  let key = match config_file.is_global_config {
    true => format!("{}\n{}", uri.as_str(), config_file.base_path.as_ref().display()),
    false => uri.as_str().to_string(),
  };
  Some((key, uri))
}

/// Gets the downloads that are each approved on their own (see
/// `UntrustedDownloadError`), which is each proxy of an `.npmrc` file, then
/// the paths on another machine together, and then the urls together.
///
/// A proxy is approved on its own so its message and decision are only about
/// it, because it receives every download with the credentials in its setting,
/// which would otherwise be easy to miss among the urls (ex. an editor cuts
/// off a long message). The paths are approved apart from the urls because
/// they're read from another machine instead of downloaded from a domain.
fn get_download_groups(downloads: &[String]) -> Vec<Vec<String>> {
  let (proxies, others): (Vec<_>, Vec<_>) = downloads.iter().cloned().partition(|download| NpmrcProxy::from_download(download).is_some());
  let (paths, urls): (Vec<_>, Vec<_>) = others.into_iter().partition(|download| is_path_download(download));
  let mut groups = proxies.into_iter().map(|proxy| vec![proxy]).collect::<Vec<_>>();
  groups.extend([paths, urls].into_iter().filter(|group| !group.is_empty()));
  groups
}

/// Gets the title of the approval of a group of downloads (see `get_download_groups`).
fn get_download_group_title(downloads: &[String]) -> &'static str {
  if downloads.iter().all(|download| NpmrcProxy::from_download(download).is_some()) {
    "Download through a proxy that isn't trusted"
  } else if downloads.iter().all(|download| is_path_download(download)) {
    "Read from another machine"
  } else {
    "Download from domains that aren't trusted"
  }
}

/// Gets the items of what to download (see `UntrustedDownloadError`), which
/// are of the kind except for the proxies of `.npmrc` files, whose files are
/// the editor's paths, and the paths on another machine, whose paths in the
/// project are the editor's paths.
/// Gets the items of the downloads, with the editor's path of each local file
/// (see `Workspace::editor_path`).
fn get_download_items(downloads: &[String], kind: ApprovalItemKind, editor_path: &impl Fn(&Path) -> PathBuf) -> Vec<ApprovalItem> {
  downloads
    .iter()
    .map(|download| match NpmrcProxy::from_download(download) {
      Some(proxy) => ApprovalItem {
        specifier: escape_text(&proxy.setting),
        kind: ApprovalItemKind::NpmrcProxy,
        registry: None,
        npmrc_uri: file_path_to_uri(&editor_path(Path::new(&proxy.npmrc_path))),
        local_uri: None,
        plugin: None,
        configuration: None,
      },
      // a path is already escaped (see `DownloadPolicy::ensure_allows_path`)
      None if is_path_download(download) => ApprovalItem {
        specifier: download.clone(),
        kind: ApprovalItemKind::NetworkPath,
        registry: None,
        npmrc_uri: None,
        local_uri: get_path_download_local_path(download).and_then(|path| file_path_to_uri(&editor_path(&path))),
        plugin: None,
        configuration: None,
      },
      None => ApprovalItem {
        specifier: escape_text(get_download_url_text(download)),
        kind,
        // the registry itself, which allows every url under it unlike another url
        registry: get_npm_registry_download_url(download).map(escape_text),
        npmrc_uri: None,
        local_uri: None,
        plugin: None,
        configuration: None,
      },
    })
    .collect()
}

/// Gets the specifier of a download like its item has it (see
/// `get_download_items`), which is the setting of a proxy.
fn get_download_specifier(download: &str) -> String {
  match NpmrcProxy::from_download(download) {
    Some(proxy) => escape_text(&proxy.setting),
    None if is_path_download(download) => download.to_string(),
    None => escape_text(get_download_url_text(download)),
  }
}

/// The version of how the approval ids are made, which changes them when it
/// does (ex. what's in them changed). What's in them is part of it, which
/// includes the item of a process plugin (see `get_process_plugin_id_part`),
/// the hash of the trusted configuration (see `get_trusted_config_hash`), the
/// text of a download (ex. `NpmrcProxy::to_download`, or the escaped text of
/// a path, whose escapes depend on the Unicode version of
/// `unicode-general-category`, see `is_escaped_in_text`), and how the parts
/// are written (see `encode_id_parts`). It also changes when what allowing the
/// same items covers does, which is never for an existing form of a download
/// (see `get_npm_registry_download`).
const APPROVAL_ID_VERSION: u32 = 1;

/// Gets an id that changes when what's approved does, which is the hash of
/// the version of how it's made, the kind, the config key, and the items (see
/// `encode_id_parts`). Everything in it is written out (ex. not with `Debug`,
/// which may change between versions of Rust) so that it's the same in
/// another server.
fn get_approval_id(kind: &str, config_key: &str, items: &[String]) -> String {
  let version = APPROVAL_ID_VERSION.to_string();
  let parts = [version.as_str(), kind, config_key].into_iter().chain(items.iter().map(|item| item.as_str()));
  // what's approved is only what's identified, so it isn't a hash that can collide
  get_sha256_checksum(encode_id_parts(parts).as_bytes())
}

/// Writes the parts of an approval id, each on its own line after its length
/// in bytes (ex. `3:abc`).
///
/// IMPORTANT: The length is what tells the parts apart, because a part may
/// have any text (ex. a newline in a path), and two different sets of items
/// would otherwise have the same id.
fn encode_id_parts<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
  let mut text = String::new();
  for part in parts {
    text.push_str(&format!("{}:{}\n", part.len(), part));
  }
  text
}

/// Gets the approval id of the process plugins of a config file.
///
/// The id has the configuration of each plugin once every one is set up. Until
/// then, it has the trusted configuration of the config file instead, which
/// each plugin's configuration is from, so it's the same however many of them
/// were set up before (ex. a plugin that another project set up).
fn get_process_plugins_approval_id(config_key: &str, unapproved: &UnapprovedProcessPlugins) -> String {
  let is_set_up = unapproved.plugins.iter().all(|plugin| plugin.approval.is_some());
  // the registry an npm plugin is from when it isn't the default one is part of what's approved too
  let mut parts = unapproved
    .plugins
    .iter()
    .map(|plugin| get_process_plugin_id_part(plugin, is_set_up))
    .collect::<Vec<_>>();
  if !is_set_up {
    parts.push(unapproved.trusted_config_hash.clone());
  }
  get_approval_id("processPlugins", config_key, &parts)
}

/// Gets the item of the approval id of a process plugin, which is the parts
/// of it written like the parts of an id (see `encode_id_parts`). They're the
/// plugin and the hash of its configuration once the plugins are set up. Until
/// then, it's `notSetUp` instead, followed by the checksum of the file of an
/// unpinned npm plugin because the hash of its configuration has it once it's
/// set up. The registry an npm plugin is from is last when it isn't the
/// default one.
fn get_process_plugin_id_part(plugin: &UnapprovedProcessPlugin, is_set_up: bool) -> String {
  let mut parts = vec![plugin.plugin.clone()];
  match plugin.approval.as_ref().filter(|_| is_set_up) {
    Some(approval) => parts.push(approval.config_hash.as_deref().unwrap_or("anyConfig").to_string()),
    None => {
      parts.push("notSetUp".to_string());
      if let Some(checksum) = &plugin.unpinned_file_checksum {
        parts.push(format!("file: {}", checksum));
      }
    }
  }
  if let Some(registry) = &plugin.registry {
    parts.push(format!("registry: {}", registry));
  }
  encode_id_parts(parts.iter().map(|part| part.as_str()))
}

/// Gets the paths to watch for changes to dprint's own files, which include
/// the canonical path of one that's a symlink (ex. a global config file in a
/// directory of dotfiles), because the watcher of its directory doesn't
/// report a change to what it points to.
fn get_dprint_files_watch_paths(environment: &impl Environment, file_paths: &[PathBuf]) -> Vec<PathBuf> {
  let mut watch_paths = file_paths.to_vec();
  for file_path in file_paths {
    if let Ok(canonical_path) = environment.canonicalize(file_path)
      && canonical_path.as_ref() != file_path.as_path()
      && !watch_paths.iter().any(|path| path == canonical_path.as_ref())
    {
      watch_paths.push(canonical_path.into_path_buf());
    }
  }
  watch_paths
}

/// A config file as an editor knows it, which is by the path it was found at
/// when it's local (see `ResolvedConfig::file_path`).
#[derive(Clone, Copy)]
struct ConfigFileRef<'a> {
  source: &'a PathSource,
  /// The path that identifies a local config file.
  file_path: Option<&'a Path>,
  is_global: bool,
  base_path: &'a Path,
}

impl<'a> ConfigFileRef<'a> {
  fn of_config(config: &'a ResolvedConfig) -> Self {
    Self {
      source: &config.source,
      file_path: config.file_path(),
      is_global: config.is_global,
      base_path: config.base_path.as_ref(),
    }
  }

  fn of_found(config_file: &'a ResolvedConfigPathWithText) -> Self {
    Self {
      source: &config_file.source,
      file_path: config_file.file_path(),
      is_global: config_file.is_global_config,
      base_path: config_file.base_path.as_ref(),
    }
  }

  /// Gets the uri that identifies the config file.
  fn uri(&self) -> Option<Uri> {
    match self.file_path {
      Some(path) => file_path_to_uri(path),
      None => path_source_to_uri(self.source),
    }
  }

  /// Gets the directory the config file is used for, which is the directory a
  /// local one was found in, apart from the global config file, which is used
  /// for each root directory (ex. drive).
  fn dir_path(&self, environment: &impl Environment) -> Option<PathBuf> {
    match self.file_path {
      Some(path) if !self.is_global => Some(get_config_file_dir(environment, path)?.to_path_buf()),
      // a remote or npm config file's patterns are relative to the directory it's used for
      _ => Some(self.base_path.to_path_buf()),
    }
  }
}

/// What a config file was resolved from, which the editor watches.
#[derive(Debug, Clone, PartialEq)]
struct ConfigDependencies {
  /// The path that identifies the config file (see `ResolvedConfig::file_path`).
  file_path: PathBuf,
  /// The canonical path of the config file when that's another path (ex. a
  /// `dprint.json` that's a symlink to a `base.json`).
  canonical_path: Option<PathBuf>,
  /// The local files it was resolved from (ex. a config file it extends).
  dependencies: Vec<PathBuf>,
}

/// Gets the canonical path of a local config file when it isn't the path
/// that identifies it (see `ResolvedConfig::found_path`).
fn get_other_canonical_path<'a>(source: &'a PathSource, found_path: Option<&Path>) -> Option<&'a Path> {
  found_path.and(source.maybe_local_path().map(|path| path.as_ref()))
}

/// Gets the uri of a config file.
fn path_source_to_uri(source: &PathSource) -> Option<Uri> {
  match source {
    PathSource::Local(local) => file_path_to_uri(local.path.as_ref()),
    PathSource::Remote(remote) => remote.url.as_str().parse().ok(),
    PathSource::Npm(npm) => npm.specifier.display().parse().ok(),
  }
}

/// Gets the paths to format a document that isn't a file as, which are in
/// the directory and named by the document's language (ex. `Untitled.ts`).
fn get_language_file_paths(dir_path: &Path, base_name: &str, document: &DocumentInfo) -> Result<Vec<PathBuf>> {
  let Some(language) = &document.language else {
    return Err(
      UnsupportedDocumentError {
        reason: NotFormattedReason::LanguageRequired,
        message: format!("Provide the language of the document to format it: {}", document.uri.as_str()),
      }
      .into(),
    );
  };
  let file_names = get_language_file_names(base_name, language);
  if file_names.is_empty() {
    return Err(UnsupportedDocumentError::unsupported(format!(
      "Could not determine a file path to format the document with language: {}",
      language.id
    )));
  }
  Ok(file_names.into_iter().map(|file_name| dir_path.join(file_name)).collect())
}

/// Gets the file names of the language in order of preference, which are
/// the editor's file extensions and file names followed by the file
/// extension dprint knows for the language.
fn get_language_file_names(base_name: &str, language: &DocumentLanguage) -> Vec<String> {
  // the editor's file names can't be paths
  let is_file_name = |name: &str| !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..";
  let mut file_names = Vec::new();
  // an extension may be provided with or without the leading period
  let extension_file_names = language
    .file_extensions
    .iter()
    .map(|extension| extension.strip_prefix('.').unwrap_or(extension))
    .filter(|extension| !extension.is_empty())
    .map(|extension| format!("{}.{}", base_name, extension));
  let editor_file_names = language.file_names.iter().cloned();
  for file_name in extension_file_names
    .chain(editor_file_names)
    .chain(get_language_file_name(base_name, &language.id))
  {
    if is_file_name(&file_name) && !file_names.contains(&file_name) {
      file_names.push(file_name);
    }
  }
  file_names
}

/// Gets the ranges of UTF-16 offsets to format in order, which are merged
/// when they overlap or touch. It's a single `None` for the whole text when a
/// range has all of it.
fn get_utf16_ranges(text: &str, ranges: &[Range<u32>]) -> Vec<Option<Range<u32>>> {
  let len = text.encode_utf16().count() as u32;
  let mut ranges = ranges
    .iter()
    .map(|range| {
      let end = std::cmp::min(range.end, len);
      std::cmp::min(range.start, end)..end
    })
    .collect::<Vec<_>>();
  ranges.sort_by_key(|range| (range.start, range.end));
  let mut merged: Vec<Range<u32>> = Vec::with_capacity(ranges.len());
  for range in ranges {
    match merged.last_mut() {
      Some(last) if range.start <= last.end => last.end = std::cmp::max(last.end, range.end),
      _ => merged.push(range),
    }
  }
  if merged.iter().any(|range| range.start == 0 && range.end == len) {
    return vec![None];
  }
  merged.into_iter().map(Some).collect()
}

/// Converts the UTF-16 offsets of a range to format to bytes, which is `None`
/// for the whole text. The offsets are clamped to the text.
fn get_byte_range(text: &str, start: u32, end: u32) -> FormatRange {
  let end = utf16_to_byte_offset(text, end);
  let start = std::cmp::min(utf16_to_byte_offset(text, start), end);
  if start == 0 && end == text.len() { None } else { Some(start..end) }
}

/// Gets the byte offset of a UTF-16 offset, which is the start of the
/// character when the offset is in the middle of one.
fn utf16_to_byte_offset(text: &str, offset: u32) -> usize {
  let mut utf16_offset = 0;
  for (byte_index, c) in text.char_indices() {
    utf16_offset += c.len_utf16() as u32;
    if utf16_offset > offset {
      return byte_index;
    }
  }
  text.len()
}

/// Gets the JSON of a value in a config file as it's written there.
fn config_map_value_to_json(value: &ConfigMapValue) -> serde_json::Value {
  fn to_json(value: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or_default()
  }

  match value {
    ConfigMapValue::KeyValue(value) => to_json(value),
    ConfigMapValue::Vec(items) => to_json(items),
    ConfigMapValue::PluginConfig(config) => {
      let mut object = config
        .properties
        .iter()
        .map(|(key, value)| (key.clone(), to_json(value)))
        .collect::<serde_json::Map<_, _>>();
      if let Some(associations) = &config.associations {
        object.insert("associations".to_string(), to_json(associations));
      }
      if config.locked {
        object.insert("locked".to_string(), serde_json::Value::Bool(true));
      }
      if !config.overrides.is_empty() {
        let overrides = config
          .overrides
          .iter()
          .map(|config_override| {
            let mut object = serde_json::Map::from_iter([("files".to_string(), to_json(&config_override.files))]);
            object.extend(config_override.properties.iter().map(|(key, value)| (key.clone(), to_json(value))));
            serde_json::Value::Object(object)
          })
          .collect();
        object.insert("overrides".to_string(), serde_json::Value::Array(overrides));
      }
      serde_json::Value::Object(object)
    }
  }
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use super::*;
  use crate::configuration::ConfigMap;
  use crate::configuration::ConfigMapValue;
  use crate::configuration::ProcessPluginApproval;
  use crate::resolution::get_trusted_config_map_hash;

  #[test]
  fn should_match_the_config_keys_of_each_directory_of_the_global_config_file() {
    let key = "file:///home/user/.config/dprint/dprint.json";
    assert!(is_config_key(key, key));
    // without a directory, it's of each directory it's used for (ex. each drive)
    assert!(is_config_key(key, &format!("{}\nfile:///C:/", key)));
    assert!(is_config_key(key, &format!("{}\nfile:///D:/", key)));
    // but not of another config file
    assert!(!is_config_key(key, &format!("{}x\nfile:///D:/", key)));
    // and with a directory, only of that directory
    let dir_key = format!("{}\nfile:///C:/", key);
    assert!(is_config_key(&dir_key, &dir_key));
    assert!(!is_config_key(&dir_key, &format!("{}\nfile:///D:/", key)));
    assert!(!is_config_key(&dir_key, key));
  }

  #[test]
  fn should_show_a_language_client_what_process_plugins_run_with() {
    let mut approval = PendingApproval {
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
      decisions: Vec::new(),
      denied: false,
      previous_decision: None,
      message: "Allow them?".to_string(),
      trusted_configuration: None,
    };
    assert_eq!(
      get_language_client_approval_message(&approval),
      r#"Allow them? They run with this configuration: {"exec":{"commands":[]}}"#
    );
    approval.items[0].plugin = None;
    approval.items[0].configuration = None;
    approval.trusted_configuration = Some(json!({ "exec": { "commands": [] } }));
    assert_eq!(
      get_language_client_approval_message(&approval),
      r#"Allow them? The plugins that weren't set up before run with this configuration: {"exec":{"commands":[]}}"#
    );
    // the configuration is from the project, so it can't change how the rest of the message reads
    let u = |hex: &str| format!("{}u{}", '\\', hex);
    approval.trusted_configuration = Some(json!({ "exec": { "a\u{202e}b": "c\u{2028}d\u{85}e", "f": "\u{301}g`" } }));
    let message = get_language_client_approval_message(&approval);
    assert_eq!(
      message,
      format!(
        r#"Allow them? The plugins that weren't set up before run with this configuration: {{"exec":{{"a{}b":"c{}d{}e","f":"{}g`"}}}}"#,
        u("202e"),
        u("2028"),
        u("0085"),
        u("0301")
      )
    );
    // which is still the same JSON
    let json_text = message.split_once("configuration: ").unwrap().1;
    assert_eq!(
      serde_json::from_str::<serde_json::Value>(json_text).unwrap(),
      approval.trusted_configuration.unwrap()
    );
    assert_eq!(
      get_escaped_json(&json!(["\u{1f600}\u{e0001}"])),
      format!("[\"\u{1f600}{}{}\"]", u("db40"), u("dc01"))
    );
  }

  #[test]
  fn should_not_change_the_approval_ids() {
    // IMPORTANT: An id is the same in another server of the same version (ex. a
    // decision for an id of the server that exited applies in the one started
    // again for the session), so increase APPROVAL_ID_VERSION when these change.
    let plugin = |config_hash: Option<Option<&str>>| UnapprovedProcessPlugin {
      plugin: "https://plugins.dprint.dev/exec-0.5.0.json@checksum".to_string(),
      registry: None,
      unpinned_file_checksum: None,
      approval: config_hash.map(|config_hash| ProcessPluginApproval {
        config_file: "/a/dprint.json".to_string(),
        plugin: "https://plugins.dprint.dev/exec-0.5.0.json@checksum".to_string(),
        config_hash: config_hash.map(|config_hash| config_hash.to_string()),
        config_key: None,
      }),
    };
    let from_registry = UnapprovedProcessPlugin {
      plugin: "npm:@dprint/exec@0.5.0".to_string(),
      registry: Some("https://npm.example.com/".to_string()),
      unpinned_file_checksum: None,
      approval: None,
    };
    let unpinned = UnapprovedProcessPlugin {
      plugin: "npm:@dprint/exec".to_string(),
      registry: None,
      unpinned_file_checksum: Some("checksum".to_string()),
      approval: None,
    };
    let trusted_config_map = ConfigMap::from([("lineWidth".to_string(), ConfigMapValue::from_i32(80))]);
    let set_up_parts = [plugin(Some(Some("1:hash"))), plugin(Some(None))]
      .into_iter()
      .map(|plugin| get_process_plugin_id_part(&plugin, true));
    // a plugin that's set up has the same part as one that isn't until every plugin is set up
    let not_set_up_parts = [plugin(Some(Some("1:hash"))), plugin(None), from_registry, unpinned]
      .into_iter()
      .map(|plugin| get_process_plugin_id_part(&plugin, false));
    let parts = set_up_parts
      .chain(not_set_up_parts)
      .chain([get_trusted_config_map_hash(&trusted_config_map)])
      .collect::<Vec<_>>();
    assert_eq!(
      parts,
      vec![
        "51:https://plugins.dprint.dev/exec-0.5.0.json@checksum\n6:1:hash\n",
        "51:https://plugins.dprint.dev/exec-0.5.0.json@checksum\n9:anyConfig\n",
        "51:https://plugins.dprint.dev/exec-0.5.0.json@checksum\n8:notSetUp\n",
        "51:https://plugins.dprint.dev/exec-0.5.0.json@checksum\n8:notSetUp\n",
        "22:npm:@dprint/exec@0.5.0\n8:notSetUp\n34:registry: https://npm.example.com/\n",
        "16:npm:@dprint/exec\n8:notSetUp\n14:file: checksum\n",
        "1:2fa6774bc8dbe963c51fb07f6539189e5d272df1e4b048319a3cd9b093e6fd48",
      ]
    );
    assert_eq!(
      get_approval_id("processPlugins", "file:///a/dprint.json", &parts),
      "15b35337acc2931482ffd77c777889b302e664300ec9d9a1cc414067fd7ac002"
    );
    assert_eq!(
      get_approval_id("download", "file:///a/dprint.json", &["https://example.com/plugin.wasm".to_string()]),
      "fbacffebf8a3f4590c1f0be91c6ed1aa05c0cf8a64a339d54c961d3c4943e2a2"
    );
  }

  #[test]
  fn should_not_have_the_same_approval_id_for_other_items() {
    let plugin = |name: &str| UnapprovedProcessPlugin {
      plugin: name.to_string(),
      registry: None,
      unpinned_file_checksum: None,
      approval: None,
    };
    let unapproved = |plugins: Vec<UnapprovedProcessPlugin>| UnapprovedProcessPlugins {
      config_file: "/a/dprint.json".to_string(),
      trusted_config_hash: "1:trusted".to_string(),
      plugins,
    };
    let get_id = |plugins| get_process_plugins_approval_id("file:///a/dprint.json", &unapproved(plugins));
    // ex. a local plugin whose path has newlines
    assert_ne!(get_id(vec![plugin("a"), plugin("b")]), get_id(vec![plugin("a\nnotSetUp\nb")]));
    assert_ne!(get_id(vec![plugin("a"), plugin("b")]), get_id(vec![plugin("a\n8:notSetUp\n1:b")]));
    let items = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    assert_ne!(
      get_approval_id("download", "file:///a/dprint.json", &items(&["/a", "/b"])),
      get_approval_id("download", "file:///a/dprint.json", &items(&["/a\n/b"]))
    );
    assert_ne!(
      get_approval_id("download", "file:///a/dprint.json\n/a", &items(&["/b"])),
      get_approval_id("download", "file:///a/dprint.json", &items(&["/a", "/b"]))
    );
  }

  #[test]
  fn should_watch_what_a_file_of_dprint_that_is_a_symlink_points_to() {
    let environment = crate::environment::TestEnvironment::new();
    environment.mk_dir_all("/config/dprint").unwrap();
    environment.mk_dir_all("/dotfiles").unwrap();
    environment.write_file("/dotfiles/dprint.json", "{}").unwrap();
    environment.write_file("/config/dprint/dprint.json", "{}").unwrap();
    let file_paths = vec![PathBuf::from("/config/dprint/dprint.json"), PathBuf::from("/config/dprint/dprint.jsonc")];
    assert_eq!(get_dprint_files_watch_paths(&environment, &file_paths), file_paths);
    // ex. a global config file in a directory of dotfiles
    environment.add_canonical_path("/config/dprint/dprint.json", "/dotfiles/dprint.json");
    let mut expected = file_paths.clone();
    expected.push(environment.canonicalize("/config/dprint/dprint.json").unwrap().into_path_buf());
    assert_eq!(get_dprint_files_watch_paths(&environment, &file_paths), expected);
  }

  #[test]
  fn should_find_a_denial_by_the_config_file_that_was_shown() {
    let uri = |path: &str| -> Uri { format!("file://{}", path).parse().unwrap() };
    let denied = DeniedDownload {
      id: String::new(),
      // ex. made by another version of dprint
      config_key: "file:///a/other-form/dprint.json".to_string(),
      downloads: vec!["https://example.com/plugin.wasm".to_string()],
      config_file: ConfigFileIdentifier {
        uri: uri("/a/dprint.json"),
        dir_uri: Some(uri("/a")),
      },
      items: Vec::new(),
    };
    let config_file = ("/a/dprint.json".to_string(), "file:///a/dprint.json".to_string());
    assert!(is_denial_for(&denied, None, None));
    assert!(!is_denial_for(&denied, Some(&config_file), None));
    assert!(is_denial_for(&denied, Some(&config_file), Some(&(uri("/a/dprint.json"), None))));
    assert!(is_denial_for(&denied, Some(&config_file), Some(&(uri("/a/dprint.json"), Some(uri("/a"))))));
    assert!(!is_denial_for(&denied, Some(&config_file), Some(&(uri("/a/dprint.json"), Some(uri("/b"))))));
    assert!(!is_denial_for(&denied, Some(&config_file), Some(&(uri("/b/dprint.json"), None))));
    // or by its key
    let config_file = ("/a/dprint.json".to_string(), "file:///a/other-form/dprint.json".to_string());
    assert!(is_denial_for(&denied, Some(&config_file), None));
  }

  #[test]
  fn should_keep_the_approval_id_of_process_plugins_until_each_one_is_set_up() {
    let plugin = |name: &str, config_hash: Option<&str>| UnapprovedProcessPlugin {
      plugin: name.to_string(),
      registry: None,
      unpinned_file_checksum: None,
      approval: config_hash.map(|config_hash| ProcessPluginApproval {
        config_file: "/a/dprint.json".to_string(),
        plugin: name.to_string(),
        config_hash: Some(config_hash.to_string()),
        config_key: None,
      }),
    };
    let unapproved = |plugins: Vec<UnapprovedProcessPlugin>| UnapprovedProcessPlugins {
      config_file: "/a/dprint.json".to_string(),
      trusted_config_hash: "1:trusted".to_string(),
      plugins,
    };
    let get_id = |plugins| get_process_plugins_approval_id("file:///a/dprint.json", &unapproved(plugins));
    let id = get_id(vec![plugin("a", None), plugin("b", None)]);
    // ex. `a` was set up by another project
    assert_eq!(get_id(vec![plugin("a", Some("1:a")), plugin("b", None)]), id);
    let set_up_id = get_id(vec![plugin("a", Some("1:a")), plugin("b", Some("1:b"))]);
    assert_ne!(set_up_id, id);
    // the configuration of a plugin is part of it once they're set up
    assert_ne!(get_id(vec![plugin("a", Some("1:a")), plugin("b", Some("1:other"))]), set_up_id);
  }

  #[test]
  fn should_approve_each_npmrc_proxy_on_its_own_with_the_editors_uri() {
    let editor_paths = EditorPaths::default();
    editor_paths.add(Path::new("/link"), Path::new("/real"));
    let proxy = NpmrcProxy {
      npmrc_path: Path::new("/real/project").join(".npmrc").display().to_string(),
      setting: "https://${TOKEN}@proxy.example".to_string(),
    };
    let other_proxy = NpmrcProxy {
      npmrc_path: Path::new("/real").join(".npmrc").display().to_string(),
      setting: "https://other.example".to_string(),
    };
    let url = "https://example.com/plugin.wasm".to_string();
    let other_url = "https://example.com/other.wasm".to_string();
    let downloads = vec![url.clone(), proxy.to_download(), other_url.clone(), other_proxy.to_download()];
    // each proxy first and then the urls together
    assert_eq!(
      get_download_groups(&downloads),
      vec![vec![proxy.to_download()], vec![other_proxy.to_download()], vec![url.clone(), other_url]]
    );
    assert_eq!(get_download_groups(std::slice::from_ref(&url)), vec![vec![url.clone()]]);
    assert_eq!(get_download_groups(&[]), Vec::<Vec<String>>::new());

    // the file of a proxy is the editor's path of it
    let items = get_download_items(&[url.clone(), proxy.to_download()], ApprovalItemKind::Plugin, &|path| editor_paths.get(path));
    assert_eq!(
      items,
      vec![
        ApprovalItem {
          specifier: url,
          kind: ApprovalItemKind::Plugin,
          registry: None,
          npmrc_uri: None,
          local_uri: None,
          plugin: None,
          configuration: None,
        },
        ApprovalItem {
          specifier: "https://${TOKEN}@proxy.example".to_string(),
          kind: ApprovalItemKind::NpmrcProxy,
          registry: None,
          npmrc_uri: Some("file:///link/project/.npmrc".parse().unwrap()),
          local_uri: None,
          plugin: None,
          configuration: None,
        },
      ]
    );
    assert_eq!(get_download_specifier(&proxy.to_download()), "https://${TOKEN}@proxy.example");
  }

  #[test]
  fn should_approve_reading_from_another_machine_on_its_own_with_the_editors_uri() {
    let editor_paths = EditorPaths::default();
    editor_paths.add(Path::new("/link"), Path::new("/real"));
    let npmrc_path = Path::new("/real/project").join(".npmrc").display().to_string();
    let path = format!(r"{} -> \\server\share\.npmrc", npmrc_path);
    let other_path = r"\\server\share\plugin.wasm".to_string();
    let url = "https://example.com/plugin.wasm".to_string();
    let proxy = NpmrcProxy {
      npmrc_path: npmrc_path.clone(),
      setting: "https://proxy.example".to_string(),
    };
    // the paths are approved apart from the urls because the settings file can't trust them
    let downloads = vec![url.clone(), path.clone(), proxy.to_download(), other_path.clone()];
    let groups = get_download_groups(&downloads);
    assert_eq!(
      groups,
      vec![vec![proxy.to_download()], vec![path.clone(), other_path.clone()], vec![url.clone()]]
    );
    assert_eq!(
      groups.iter().map(|group| get_download_group_title(group)).collect::<Vec<_>>(),
      vec![
        "Download through a proxy that isn't trusted",
        "Read from another machine",
        "Download from domains that aren't trusted"
      ]
    );

    // a path's kind is its own, whatever needs it, with the editor's path of it in the project
    let items = get_download_items(&[path.clone(), other_path.clone()], ApprovalItemKind::Config, &|path| editor_paths.get(path));
    assert_eq!(
      items,
      vec![
        ApprovalItem {
          specifier: path.clone(),
          kind: ApprovalItemKind::NetworkPath,
          registry: None,
          npmrc_uri: None,
          local_uri: Some("file:///link/project/.npmrc".parse().unwrap()),
          plugin: None,
          configuration: None,
        },
        ApprovalItem {
          specifier: other_path.clone(),
          kind: ApprovalItemKind::NetworkPath,
          registry: None,
          npmrc_uri: None,
          local_uri: None,
          plugin: None,
          configuration: None,
        },
      ]
    );
    assert_eq!(get_download_specifier(&path), path);
  }

  #[test]
  fn should_escape_the_specifiers_from_the_project() {
    let editor_paths = EditorPaths::default();
    let proxy = NpmrcProxy {
      npmrc_path: "/project/.npmrc".to_string(),
      setting: "https://proxy.example/\u{202e}x".to_string(),
    };
    let url = "https://example.com/\u{2028}".to_string();
    let items = get_download_items(&[url.clone(), proxy.to_download()], ApprovalItemKind::Plugin, &|path| editor_paths.get(path));
    assert_eq!(
      items.iter().map(|item| item.specifier.as_str()).collect::<Vec<_>>(),
      vec![r"https://example.com/\u{2028}", r"https://proxy.example/\u{202e}x"]
    );
    assert_eq!(get_download_specifier(&proxy.to_download()), r"https://proxy.example/\u{202e}x");
  }
}
