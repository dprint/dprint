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
use crate::commands::process_plugin_approval::store_process_plugin_action;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::commands::scope_container::get_config_file_key;
use crate::configuration::ConfigMapValue;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::SessionPermissions;
use crate::configuration::forget_editor_approvals;
use crate::configuration::get_editor_approvals_file_path;
use crate::configuration::get_settings_file_path;
use crate::configuration::is_dot_config_dir;
use crate::configuration::read_editor_approval_denials;
use crate::configuration::read_editor_approvals;
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
use crate::utils::get_bytes_hash;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;
use crate::utils::get_sha256_checksum;
use crate::utils::get_untrusted_download_message;

mod config_diagnostics;
mod editor_paths;
pub mod messages;

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
  /// needs to be approved now for an approval that isn't pending anymore.
  /// Only the most recent ones are kept.
  approval_files: RefCell<IndexMap<String, ApprovalTarget>>,
  /// The config files that were used by their key, which are resolved again
  /// when dprint's own files change.
  used_configs: RefCell<HashMap<String, ResolvedConfigPathWithText>>,
  /// The keys of the config files that failed to resolve, which tells the
  /// editor extension once they resolve again.
  failed_configs: RefCell<HashSet<String>>,
  /// The path of each config file uri and the local files it was resolved
  /// from the last time it resolved, which are kept when it fails to resolve
  /// so that fixing one of them resolves it again.
  config_dependencies: RefCell<HashMap<String, (PathBuf, Vec<PathBuf>)>>,
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
}

/// The plugins of a config file the last time they were known.
struct LastPluginInfos {
  /// What identifies the plugins of the config file they're for, which they
  /// aren't for anymore once a plugin was added or changed.
  plugin_references: Vec<String>,
  plugins: Vec<PluginInfo>,
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

/// Downloads the user denied for a config file this session.
struct DeniedDownload {
  /// The key of the config file (see `get_config_key`).
  config_key: String,
  /// The config file the user was shown.
  config_file: ConfigFileIdentifier,
  /// What the user was shown as the approval's items.
  items: Vec<ApprovalItem>,
}

/// What needs to be approved for a config file.
#[derive(Clone)]
struct StoredApproval {
  /// The key of the config file it's for (see `get_config_key`).
  config_key: String,
  /// What the config file it's for is resolved again for once it's approved,
  /// which is the config file itself by its canonicalized path unless it
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
  /// The key of the config file (see `get_config_key`) and the uri of its
  /// canonicalized path when it's known.
  config_key: Option<(String, Uri)>,
  /// What the config file is resolved for by its canonicalized path when it can be.
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
      prompted_approvals: Default::default(),
      has_notified_no_config: Default::default(),
      pending_config_changes: Default::default(),
      has_settings: Default::default(),
      canonical_workspace_folders: Default::default(),
      had_global_config: Default::default(),
      registered_watch_files: Default::default(),
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

  pub async fn shutdown(&self) {
    self.stopped.cancel();
    self.scope_container.shutdown().await;
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
      let changes = std::mem::take(&mut *workspace.pending_config_changes.borrow_mut());
      for change in changes {
        // a config file that was resolved from the file resolves to something else now
        for config_file_path in workspace.get_dependent_config_files(&change.file_path) {
          workspace.handle_config_file_change(&config_file_path, ConfigFileChange::Changed).await;
        }
        // the config file the server was started with is the only one
        if workspace.scope_container.has_config_override() {
          if workspace.is_config_override_path(&change.file_path) {
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
    // the config files are known by their canonicalized paths
    let file_path = &self.canonicalize(file_path.to_path_buf());
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
    let file_path = self.canonicalize(file_path.to_path_buf());
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
  /// file they extend).
  fn get_dependent_config_files(&self, file_path: &Path) -> Vec<PathBuf> {
    let file_path = self.canonicalize(file_path.to_path_buf());
    let mut config_file_paths = Vec::new();
    for (config_file_path, dependencies) in self.config_dependencies.borrow().values() {
      if dependencies.contains(&file_path) && !config_file_paths.contains(config_file_path) {
        config_file_paths.push(config_file_path.clone());
      }
    }
    config_file_paths
  }

  /// Gets whether the config file was resolved before.
  fn was_resolved(&self, file_path: &Path) -> bool {
    let file_path = self.canonicalize(file_path.to_path_buf()).display().to_string();
    self
      .resolved_configs
      .borrow()
      .keys()
      .chain(self.config_errors.borrow().keys())
      .chain(self.failed_configs.borrow().iter())
      .any(|key| key.split('\n').next() == Some(file_path.as_str()))
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
        return Ok(ConfigInfoResult {
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
          .map(|(_, dependencies)| dependencies.iter().filter_map(|path| self.editor_uri(path)).collect())
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
      .map(|config| config.local_dependencies.iter().filter_map(|path| self.editor_uri(path)).collect())
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
    // they're of other plugins once a plugin was added or changed
    if let Some(last) = self.last_plugin_infos.borrow().get(&get_config_key(config))
      && last.plugin_references == get_plugin_references(config)
    {
      return Some(last.plugins.clone());
    }
    // a plugin's information is in the plugin cache, which is kept across sessions
    config
      .plugins
      .iter()
      .map(|plugin| {
        let info = self.scope_container.get_set_up_plugin_info(plugin)?;
        let file_matching = self.scope_container.get_last_file_matching_info(plugin)?;
        Some(get_plugin_info(info, file_matching))
      })
      .collect()
  }

  /// Remembers the plugins of the config file for when they can't be resolved
  /// (ex. the config file has an error or the user needs to approve them).
  async fn store_plugin_infos(&self, config: &ResolvedConfig, scope: &PluginsScope<TEnvironment>, plugins: &[PluginInfo]) {
    self.last_plugin_infos.borrow_mut().insert(
      get_config_key(config),
      LastPluginInfos {
        plugin_references: get_plugin_references(config),
        plugins: plugins.to_vec(),
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
      if let Err(err) = self.scope_container.store_file_matching_info(plugin, &plugin_with_config.file_matching).await {
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
      ApprovalTarget::ConfigFile { file_path, dir_path } => match self.load_config_file(file_path, dir_path.as_deref())? {
        Some(config_file) => self.resolve_found_config(config_file).await,
        None => Ok(DocumentConfig::NotFound),
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
            Some(dir_path) => self.canonicalize(dir_path),
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
            return Ok((
              ApprovalTarget::ConfigFile {
                file_path: config_file_path,
                dir_path,
              },
              DocumentConfig::Failed(ConfigFailure {
                config_file: None,
                config_key: None,
                config_target: None,
                reason: NotFormattedReason::ConfigError,
                message: format!("{:#}", err),
                downloads: Vec::new(),
              }),
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
  /// which may not exist right now.
  fn is_config_override_uri(&self, uri: &Uri) -> bool {
    uri_to_file_path(uri).is_some_and(|path| self.is_config_override_path(&path))
  }

  /// Gets whether the path is of the config file the server was started with.
  fn is_config_override_path(&self, path: &Path) -> bool {
    match self.scope_container.config_override_path() {
      Some(override_path) => self.canonicalize(path.to_path_buf()) == self.canonicalize(override_path),
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
    let config_file = self.get_config_file_info(&scope);
    let outcome = self.format_with_scope(scope, request, document_file, target, use_global_config, token).await?;
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
  /// to be approved now. That has what was denied when it was denied, and
  /// otherwise what else needs to be approved (ex. a config file that a
  /// downloaded one extends), which is empty once nothing does. Approving a
  /// process plugin that isn't set up allows setting it up, which sets it up.
  ///
  /// The decision is used once this is called, even when the request is
  /// cancelled while what was allowed is set up.
  ///
  /// It's with what to tell the user about the decision (ex. it couldn't be
  /// stored).
  pub async fn approve(self: &Rc<Self>, id: &str, decision: ApprovalDecision) -> Result<(ConfigApprovals, Option<Notice>)> {
    let stored = self.approvals.borrow().values().flatten().find(|stored| stored.approval.id == id).cloned();
    let Some(stored) = stored else {
      // say what needs to be approved now when the config file is known
      let target = self.approval_files.borrow().get(id).cloned();
      let now = match target {
        // it's still an approval that changed when the config file can't be resolved right now
        Some(target) => match self.resolve_approval_target(&target).await {
          Ok(config) => Some(self.get_approvals_and_status(config, &target)),
          Err(_) => None,
        },
        None => None,
      };
      return Err(
        ApprovalChangedError {
          id: id.to_string(),
          current: now.map(|(config_file, approvals, status)| ConfigApprovals {
            config_file,
            approvals,
            status,
          }),
        }
        .into(),
      );
    };
    if !stored.approval.decisions.iter().any(|option| option.id == decision) {
      return Err(InvalidParamsError(format!("The decision {:?} isn't one of the approval's decisions.", decision)).into());
    }
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
        } else {
          self.denied_downloads.borrow_mut().insert(
            stored.approval.id.clone(),
            DeniedDownload {
              config_key: stored.config_key.clone(),
              config_file: ConfigFileIdentifier {
                uri: stored.approval.config_file.uri.clone(),
                dir_uri: Some(stored.approval.config_file.dir_uri.clone()),
              },
              items: stored.approval.items.clone(),
            },
          );
        }
      }
      ApprovalSubject::ProcessPlugins(unapproved) => {
        let title = get_process_plugin_action_title(decision);
        if let Err(err) = store_process_plugin_action(&self.environment, &self.session_permissions, unapproved, Some(title)).await {
          let message = format!("Failed storing the decision for the process plugins: {:#}", err);
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
    let config_approvals = ConfigApprovals {
      config_file,
      approvals,
      status,
    };
    Ok((config_approvals, notice))
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

  /// Gets what a config file is resolved for by its canonicalized path, which
  /// is `None` when it's resolved for a file instead (ex. it's remote or the
  /// one the server was started with).
  fn get_config_file_target(&self, source: &PathSource, is_global: bool, base_path: &Path) -> Option<ApprovalTarget> {
    if self.scope_container.has_config_override() {
      return None;
    }
    Some(ApprovalTarget::ConfigFile {
      file_path: source.maybe_local_path()?.as_ref().to_path_buf(),
      dir_path: is_global.then(|| base_path.to_path_buf()),
    })
  }

  /// Forgets what needs to be approved for the config file of the target when
  /// it doesn't exist anymore, which isn't approvable then.
  fn forget_missing_config_approvals(&self, target: &ApprovalTarget) {
    if let ApprovalTarget::ConfigFile { file_path, .. } = target
      && let Some(uri) = file_path_to_uri(&self.canonicalize(file_path.clone()))
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
    let config_file = match config_file {
      Some((uri, dir_uri)) => Some(self.get_decisions_config_file(&uri, dir_uri.as_ref())?),
      None => None,
    };
    // the downloads allowed for the session stay allowed (ex. they're in the cache already)
    let forgotten = self
      .get_decisions(config_file.as_ref())
      .into_iter()
      .filter(|decision| decision.config_file.is_some())
      .collect::<Vec<_>>();
    let is_config_file = |source: &str| config_file.as_ref().is_none_or(|(config_source, _)| config_source == source);
    self.session_permissions.forget_process_plugin_decisions(&is_config_file);
    self
      .denied_downloads
      .borrow_mut()
      .retain(|_, denied| config_file.as_ref().is_some_and(|(_, key)| *key != denied.config_key));
    let stored_result = self.forget_stored_decisions(&is_config_file).await;
    self.resolve_used_configs_again().await;
    // what was forgotten for the session still is when the stored decisions can't be
    let notice = stored_result.err().map(|err| {
      let message = format!("Failed forgetting the decisions that were stored: {:#}", err);
      log_warn!(self.environment, "{}", message);
      new_warning_notice(message)
    });
    Ok((forgotten, notice))
  }

  /// Gets what the user decided for the config file, or for every config file
  /// when `None`, which says whether the settings file allows every process
  /// plugin.
  pub fn decisions(&self, config_file: Option<(Uri, Option<Uri>)>) -> Result<(Vec<Decision>, bool)> {
    let config_file = match config_file {
      Some((uri, dir_uri)) => Some(self.get_decisions_config_file(&uri, dir_uri.as_ref())?),
      None => None,
    };
    let allow_process_plugins = self.session_permissions.read(&self.environment).allow_process_plugins == Some(true);
    Ok((self.get_decisions(config_file.as_ref()), allow_process_plugins))
  }

  /// Gets what the user decided for the config file as the process plugin
  /// decisions have it with its key, or for every config file when `None`.
  fn get_decisions(&self, config_file: Option<&(String, String)>) -> Vec<Decision> {
    let is_config_file = |source: &str| config_file.is_none_or(|(config_source, _)| config_source == source);
    // the stored decisions are read even when the settings allow every process plugin
    // the denials of a file that can't be read are still used, so they're provided too
    let stored = read_editor_approvals(&self.environment)
      .map(|approvals| approvals.process_plugin_decisions)
      .unwrap_or_else(|_| read_editor_approval_denials(&self.environment));
    let session = self.session_permissions.session_process_plugin_decisions();
    // allowing a plugin that isn't set up is a grant until it's set up
    let grants = self.session_permissions.process_plugin_set_up_grants();
    let candidates = stored
      .iter()
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
    // the most recent decision for a plugin of a config file is what's used, which is the
    // session's or a grant on a tie like when formatting
    let mut plugin_decisions: Vec<(String, String, bool, bool, u64)> = Vec::new();
    for (config_file, plugin, allows, always, decided_at) in candidates {
      if !is_config_file(config_file) {
        continue;
      }
      let existing = plugin_decisions.iter().position(|(c, p, ..)| c == config_file && p == plugin);
      if let Some(index) = existing {
        if plugin_decisions[index].4 > decided_at {
          continue;
        }
        plugin_decisions.remove(index);
      }
      plugin_decisions.push((config_file.clone(), plugin.clone(), allows, always, decided_at));
    }
    let global_config_path = self.get_global_config_path();
    let mut decisions = Vec::new();
    for (config_file, plugin, allows, always, _) in plugin_decisions {
      // it's a path unless it's a remote or npm config file
      let is_remote = ["http://", "https://", "npm:"].iter().any(|prefix| config_file.starts_with(prefix));
      let config_file = match is_remote {
        true => config_file.parse().ok().map(|uri| ConfigFileIdentifier { uri, dir_uri: None }),
        false => {
          let path = Path::new(&config_file);
          // the decisions of the global config file are for each directory it's used for
          let dir_uri = match global_config_path.as_deref() == Some(path) {
            true => None,
            false => get_config_file_dir(&self.environment, path).and_then(|dir_path| self.editor_uri(dir_path)),
          };
          self.editor_uri(path).map(|uri| ConfigFileIdentifier { uri, dir_uri })
        }
      };
      decisions.push(Decision {
        kind: ApprovalKind::ProcessPlugins,
        config_file,
        specifiers: vec![plugin],
        items: None,
        allows,
        always,
      });
    }
    for denied in self.denied_downloads.borrow().values() {
      if config_file.is_none_or(|(_, key)| *key == denied.config_key) {
        decisions.push(Decision {
          kind: ApprovalKind::Downloads,
          config_file: Some(denied.config_file.clone()),
          specifiers: denied.items.iter().map(|item| item.specifier.clone()).collect(),
          items: Some(denied.items.clone()),
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
        items: item.map(|item| vec![item.clone()]),
        allows: true,
        always: false,
      }
    }));
    decisions
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
      let Some(file_path) = uri_to_file_path(uri).filter(|path| is_config_file_path(&self.environment, path)) else {
        return Err(InvalidParamsError(format!("The configFile isn't a config file: {}", uri.as_str())).into());
      };
      match self.load_config_file(&file_path, dir_path.as_deref())? {
        Some(config_file) => Some(config_file),
        // a deleted config file's decisions are still forgotten
        None => {
          let file_path = self.canonicalize(file_path);
          let key = file_path_to_uri(&file_path).map(|uri| uri.as_str().to_string()).unwrap_or_default();
          return Ok((file_path.display().to_string(), key));
        }
      }
    };
    Ok(match config_file {
      Some(config_file) => {
        let key = get_found_config_key(&config_file)
          .map(|(key, _)| key)
          .unwrap_or_else(|| config_file.source.display());
        (config_file.source.display(), key)
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
      let items = get_download_items(&urls, ApprovalItemKind::Plugin, &self.editor_paths);
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
    let config_target = self.get_config_file_target(&config.source, config.is_global, config.base_path.as_ref());
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
      specifier: unapproved_plugin.plugin.clone(),
      kind: ApprovalItemKind::Plugin,
      registry: unapproved_plugin.registry.clone(),
      npmrc_uri: None,
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
      let items = get_download_items(&urls, ApprovalItemKind::Config, &self.editor_paths);
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
            let denied = self.denied_downloads.borrow().contains_key(&id);
            PendingApproval {
              denied,
              // a download is only denied for the session
              previous_decision: denied.then_some(ApprovalDecision::Deny),
              id,
              kind: ApprovalKind::Downloads,
              // a proxy is approved on its own (see `get_download_groups`)
              title: match urls.iter().all(|download| NpmrcProxy::from_download(download).is_some()) {
                true => "Download through a proxy that isn't trusted".to_string(),
                false => "Download plugins from domains that aren't trusted".to_string(),
              },
              config_file: config_file.clone(),
              trusted_configuration: None,
              items,
              // a download is only allowed for the session
              decisions: get_decision_options(ApprovalKind::Downloads, &[ApprovalDecision::Allow, ApprovalDecision::Deny]),
              message: self.get_untrusted_download_message(urls),
              id_before_set_up: None,
            }
          }
          ApprovalSubject::ProcessPlugins(unapproved) => {
            // the configuration of each plugin is part of what's approved, which is the
            // trusted configuration of the config file for a plugin that isn't set up,
            // and so is the registry an npm plugin is from when it isn't the default one
            let mut parts = unapproved.plugins.iter().map(get_process_plugin_id_part).collect::<Vec<_>>();
            if unapproved.plugins.iter().any(|plugin| plugin.approval.is_none()) {
              parts.push(unapproved.trusted_config_hash.clone());
            }
            // a server started before may have provided it before the plugins were set up, which
            // allowed running them with the same trusted configuration
            let id_before_set_up = unapproved.plugins.iter().any(|plugin| plugin.approval.is_some()).then(|| {
              let mut parts = unapproved
                .plugins
                .iter()
                .map(|plugin| {
                  get_process_plugin_id_part(&UnapprovedProcessPlugin {
                    approval: None,
                    ..plugin.clone()
                  })
                })
                .collect::<Vec<_>>();
              parts.push(unapproved.trusted_config_hash.clone());
              get_approval_id("processPlugins", &config_key, &parts)
            });
            PendingApproval {
              id: get_approval_id("processPlugins", &config_key, &parts),
              id_before_set_up,
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
            }
          }
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
      approval_files.insert(stored.approval.id.clone(), stored.config_target.clone());
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
        match workspace.approve(&approval.id, decision).await {
          Ok((result, notice)) => {
            // a language client's user is told with a message
            if let Some(notice) = notice {
              workspace.client.show_message(MessageType::WARNING, notice.message);
            }
            pending.extend(workspace.take_unprompted_approvals(result.approvals.into_iter()));
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
    let config_file = scope.config.as_ref().map(|config| config.source.display()).unwrap_or_default();
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
        &config.source,
        get_config_diagnostics(config_text.as_deref(), scope.global_config_diagnostics(), &plugin_diagnostics),
      );
    }
    let actions = scope
      .config
      .as_ref()
      .and_then(|config| self.editor_source_uri(&config.source))
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

  /// Resolves the plugins of a config file that was found.
  async fn resolve_found_config(self: &Rc<Self>, config_file: ResolvedConfigPathWithText) -> Result<DocumentConfig<TEnvironment>> {
    let key = get_config_file_key(&config_file);
    self.used_configs.borrow_mut().insert(key.clone(), config_file.clone());
    let failed = |reason: NotFormattedReason, message: String, downloads: Vec<String>| {
      DocumentConfig::Failed(ConfigFailure {
        config_file: self.new_config_file_info(&config_file.source, config_file.is_global_config, config_file.base_path.as_ref()),
        config_key: get_found_config_key(&config_file),
        config_target: self.get_config_file_target(&config_file.source, config_file.is_global_config, config_file.base_path.as_ref()),
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
          let downloads = vec![err.download.clone()];
          return Ok(failed(
            NotFormattedReason::ApprovalRequired,
            self.get_untrusted_download_message(&downloads),
            downloads,
          ));
        }
        self.show_config_error(key.clone(), &err);
        self.checked_scopes.borrow_mut().remove(&key);
        self.publish_config_diagnostics(&config_file.source, vec![get_config_error_diagnostic(Some(&config_file.content), &err)]);
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
      &config_file.source,
      get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &[]),
    );
    let workspace = self.clone();
    let key = key.to_string();
    let source = config_file.source.clone();
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
      workspace.publish_config_diagnostics(
        &source,
        get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &plugin_diagnostics),
      );
    });
  }

  /// Publishes the problems with the config file when they're different from
  /// the ones that were published before.
  fn publish_config_diagnostics(&self, source: &PathSource, diagnostics: Vec<ConfigDiagnostic>) {
    if let (Some(uri), Some(editor_uri)) = (path_source_to_uri(source), self.editor_source_uri(source)) {
      self.publish_diagnostics(&uri, editor_uri, diagnostics);
    }
  }

  /// Publishes the problems with the config file of the canonicalized uri to
  /// the editor's uri of it, which clears the ones published to another
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
    if let (Some(uri), Some(config_file_path)) = (path_source_to_uri(&config_file.source), config_file.source.maybe_local_path()) {
      self.config_dependencies.borrow_mut().insert(
        uri.as_str().to_string(),
        (config_file_path.as_ref().to_path_buf(), config.local_dependencies.clone()),
      );
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
    self.register_watch_files();
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
      .any(|urls| self.denied_downloads.borrow().contains_key(&get_approval_id("download", &config_key, urls)));
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
    for (config_file_path, dependencies) in self.config_dependencies.borrow().values() {
      for file_path in std::iter::once(config_file_path).chain(dependencies.iter()) {
        if Some(file_path) != global_config_path.as_ref() && !file_paths.contains(file_path) {
          file_paths.push(file_path.clone());
        }
      }
    }
    // the editor watches them by its paths of them
    let mut file_paths = file_paths.iter().map(|path| self.editor_paths.get(path)).collect::<Vec<_>>();
    file_paths.sort();
    if *registered != file_paths {
      self.client.register_watched_files(&file_paths);
      *registered = file_paths;
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
    let mut watcher = match self.environment.watch_files(&file_paths) {
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
        // they're watched once they can be (ex. the directory was created)
        if watcher.is_none() {
          watcher = workspace.environment.watch_files(&file_paths).ok();
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
      // read it again in case it changed (ex. the global config file)
      let config_file = if previous.is_global_config {
        resolve_global_config_path_and_text_at_path_root(&self.environment, previous.base_path.as_ref())
          .ok()
          .flatten()
      } else {
        previous
          .source
          .maybe_local_path()
          .and_then(|path| self.scope_container.load_config_file(path.as_ref()).ok().flatten())
      };
      let Some(config_file) = config_file else {
        // the editor isn't told when the global config file is deleted
        if previous.is_global_config
          && let Some(file_path) = previous.source.maybe_local_path()
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
      let target = match config_file.source.maybe_local_path() {
        Some(path) => ApprovalTarget::ConfigFile {
          file_path: path.as_ref().to_path_buf(),
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

  /// Forgets a config file that was deleted, which isn't resolved again.
  fn forget_config_file(&self, file_path: &Path) {
    let is_config_file = |key: &String| key.split('\n').next().is_some_and(|source| Path::new(source) == file_path);
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
  fn load_config_file(&self, file_path: &Path, dir_path: Option<&Path>) -> Result<Option<ResolvedConfigPathWithText>> {
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
    self.new_config_file_info(&config.source, config.is_global, config.base_path.as_ref())
  }

  fn new_config_file_info(&self, source: &PathSource, is_global: bool, base_path: &Path) -> Option<ConfigFileInfo> {
    Some(ConfigFileInfo {
      uri: self.editor_source_uri(source)?,
      dir_uri: self.editor_uri(&get_config_dir_path(&self.environment, source, is_global, base_path)?)?,
      kind: self.get_config_file_kind(is_global),
      in_workspace: !is_global && source.maybe_local_path().is_some_and(|path| self.is_in_workspace(path.as_ref())),
    })
  }

  /// Gets the uri of a canonicalized path as the editor knows it (ex. through
  /// a symlink), which is what the server provides.
  fn editor_uri(&self, path: &Path) -> Option<Uri> {
    file_path_to_uri(&self.editor_paths.get(path))
  }

  /// Gets the uri of a config file as the editor knows it.
  fn editor_source_uri(&self, source: &PathSource) -> Option<Uri> {
    match source.maybe_local_path() {
      Some(path) => self.editor_uri(path.as_ref()),
      None => path_source_to_uri(source),
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
          npmrc_path: self.editor_paths.get(Path::new(&proxy.npmrc_path)).display().to_string(),
          ..proxy
        }
        .to_download(),
        None => download.clone(),
      })
      .collect::<Vec<_>>();
    get_untrusted_download_message(&downloads)
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
    Some(new_identified_config_changed(
      self.editor_source_uri(&config.source)?,
      self.editor_uri(&get_config_dir_path(
        &self.environment,
        &config.source,
        config.is_global,
        config.base_path.as_ref(),
      )?)?,
      change,
    ))
  }

  /// Gets the notification that a config file that was found changed.
  fn new_found_config_changed(&self, config_file: &ResolvedConfigPathWithText) -> Option<ConfigChangedParams> {
    let dir_path = get_config_dir_path(
      &self.environment,
      &config_file.source,
      config_file.is_global_config,
      config_file.base_path.as_ref(),
    )?;
    Some(new_identified_config_changed(
      self.editor_source_uri(&config_file.source)?,
      self.editor_uri(&dir_path)?,
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
  fn get_failure_not_formatted(&self, failure: &ConfigFailure, target: &ApprovalTarget) -> NotFormatted {
    let actions = match (&failure.config_file, failure.reason) {
      (Some(config_file), NotFormattedReason::ConfigError) => vec![open_config_file_action(&config_file.uri)],
      _ => Vec::new(),
    };
    let not_formatted = NotFormatted::new(failure.reason, Some(failure.message.clone()), self.get_failure_approvals(failure, target)).with_actions(actions);
    // what it formats isn't known until what it extends is downloaded
    if failure.reason == NotFormattedReason::ApprovalRequired {
      not_formatted.unclaimed()
    } else {
      not_formatted
    }
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
        notebook_path: Some(notebook_path),
      });
    }
    if is_untitled_uri(&document.uri) {
      return Ok(DocumentFile {
        candidates: get_language_file_paths(&self.get_untitled_dir_path(&document.uri)?, "Untitled", document)?,
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
    let notebook_path = document_file.notebook_path.map(|path| self.canonicalize(path));
    let mut first = None;
    for candidate in document_file.candidates {
      let file_path = self.canonicalize(candidate);
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
  fn canonicalize(&self, path: PathBuf) -> PathBuf {
    let canonical_path = self
      .environment
      .canonicalize_maybe_not_exists(&path)
      .map(|p| p.into_path_buf())
      .unwrap_or(path.clone());
    // the paths the server provides are the editor's (ex. a workspace opened through a symlink)
    if canonical_path != path && self.editor_paths.get(&canonical_path) != path {
      self.add_editor_dir(&path, &canonical_path);
    }
    canonical_path
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
      self.editor_paths.add(editor_dir, canonical_dir);
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
      id: *decision,
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
    message.push_str(&format!(" They run with this configuration: {}", serde_json::Value::Object(configuration)));
  }
  if let Some(trusted_configuration) = &approval.trusted_configuration {
    message.push_str(&format!(
      " The plugins that weren't set up before run with this configuration: {}",
      trusted_configuration
    ));
  }
  message
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

/// Gets the directory whose files the config file formats.
fn get_config_dir_path(environment: &impl Environment, source: &PathSource, is_global: bool, base_path: &Path) -> Option<PathBuf> {
  if is_global {
    // the global config file is used for each root directory (ex. drive)
    return Some(base_path.to_path_buf());
  }
  match source {
    PathSource::Local(local) => Some(get_config_file_dir(environment, local.path.as_ref())?.to_path_buf()),
    // its patterns are relative to the directory it's used for
    PathSource::Remote(_) | PathSource::Npm(_) => Some(base_path.to_path_buf()),
  }
}

/// Gets whether the file is a config file that's found for a directory by
/// its name, like dprint finds them for a file.
pub fn is_config_file_path(environment: &impl Environment, file_path: &Path) -> bool {
  let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
    return false;
  };
  match file_path.parent() {
    // only these are found in a `.config` directory, which isn't the home directory's
    Some(dir_path) if is_dot_config_dir(environment, dir_path) => matches!(file_name, "dprint.json" | "dprint.jsonc"),
    _ => matches!(file_name, "dprint.json" | "dprint.jsonc" | ".dprint.json" | ".dprint.jsonc"),
  }
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
  let key = path_source_to_uri(&config.source)
    .map(|uri| uri.as_str().to_string())
    .unwrap_or_else(|| config.source.display());
  match config.is_global {
    true => format!("{}\n{}", key, config.base_path.as_ref().display()),
    false => key,
  }
}

/// Gets the key of a config file that was found (see `get_config_key`) with
/// the uri of its canonicalized path.
fn get_found_config_key(config_file: &ResolvedConfigPathWithText) -> Option<(String, Uri)> {
  let uri = path_source_to_uri(&config_file.source)?;
  let key = match config_file.is_global_config {
    true => format!("{}\n{}", uri.as_str(), config_file.base_path.as_ref().display()),
    false => uri.as_str().to_string(),
  };
  Some((key, uri))
}

/// Gets the downloads that are each approved on their own (see
/// `UntrustedDownloadError`), which is each proxy of an `.npmrc` file and then
/// the urls together.
///
/// A proxy is approved on its own so its message and decision are only about
/// it, because it receives every download with the credentials in its setting,
/// which would otherwise be easy to miss among the urls (ex. an editor cuts
/// off a long message).
fn get_download_groups(downloads: &[String]) -> Vec<Vec<String>> {
  let (proxies, urls): (Vec<_>, Vec<_>) = downloads.iter().cloned().partition(|download| NpmrcProxy::from_download(download).is_some());
  let mut groups = proxies.into_iter().map(|proxy| vec![proxy]).collect::<Vec<_>>();
  if !urls.is_empty() {
    groups.push(urls);
  }
  groups
}

/// Gets the items of what to download (see `UntrustedDownloadError`), which
/// are of the kind except for the proxies of `.npmrc` files, whose files are
/// the editor's paths.
fn get_download_items(downloads: &[String], kind: ApprovalItemKind, editor_paths: &EditorPaths) -> Vec<ApprovalItem> {
  downloads
    .iter()
    .map(|download| match NpmrcProxy::from_download(download) {
      Some(proxy) => ApprovalItem {
        specifier: proxy.setting,
        kind: ApprovalItemKind::NpmrcProxy,
        registry: None,
        npmrc_uri: file_path_to_uri(&editor_paths.get(Path::new(&proxy.npmrc_path))),
        plugin: None,
        configuration: None,
      },
      None => ApprovalItem {
        specifier: download.clone(),
        kind,
        registry: None,
        npmrc_uri: None,
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
    Some(proxy) => proxy.setting,
    None => download.to_string(),
  }
}

/// The version of how the approval ids are made, which changes them when it
/// does (ex. what's in them changed). What's in them is part of it, which
/// includes the item of a process plugin (see `get_process_plugin_id_part`),
/// the hash of the trusted configuration (see `get_trusted_config_hash`), and
/// the text of a download (ex. `NpmrcProxy::to_download`).
const APPROVAL_ID_VERSION: u32 = 1;

/// Gets an id that changes when what's approved does, which is the hash of
/// the version of how it's made, the kind, the config key, and the items, each
/// on its own line. Everything in it is written out (ex. not with `Debug`,
/// which may change between versions of Rust) so that it's the same in
/// another server.
fn get_approval_id(kind: &str, config_key: &str, items: &[String]) -> String {
  let mut text = format!("{}\n{}\n{}", APPROVAL_ID_VERSION, kind, config_key);
  for item in items {
    text.push('\n');
    text.push_str(item);
  }
  // what's approved is only what's identified, so it isn't a hash that can collide
  get_sha256_checksum(text.as_bytes())
}

/// Gets the item of the approval id of a process plugin, which is the plugin
/// and the hash of its configuration on the next line, or `notSetUp` when its
/// configuration isn't known until it's set up, followed by the registry an
/// npm plugin is from on another line when it isn't the default one.
fn get_process_plugin_id_part(plugin: &UnapprovedProcessPlugin) -> String {
  let config = match plugin.approval.as_ref().map(|approval| approval.config_hash.as_deref()) {
    None => "notSetUp",
    Some(None) => "anyConfig",
    Some(Some(config_hash)) => config_hash,
  };
  match &plugin.registry {
    Some(registry) => format!("{}\n{}\nregistry: {}", plugin.plugin, config, registry),
    None => format!("{}\n{}", plugin.plugin, config),
  }
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
      },
      items: vec![ApprovalItem {
        specifier: "https://plugins.dprint.dev/exec.json@checksum".to_string(),
        kind: ApprovalItemKind::Plugin,
        registry: None,
        npmrc_uri: None,
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
      id_before_set_up: None,
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
  }

  #[test]
  fn should_not_change_the_approval_ids() {
    // IMPORTANT: The editor extensions send a decision again for the same id
    // after the server restarts, so increase APPROVAL_ID_VERSION when these change.
    let plugin = |config_hash: Option<Option<&str>>| UnapprovedProcessPlugin {
      plugin: "https://plugins.dprint.dev/exec-0.5.0.json@checksum".to_string(),
      registry: None,
      approval: config_hash.map(|config_hash| ProcessPluginApproval {
        config_file: "/a/dprint.json".to_string(),
        plugin: "https://plugins.dprint.dev/exec-0.5.0.json@checksum".to_string(),
        config_hash: config_hash.map(|config_hash| config_hash.to_string()),
      }),
    };
    let from_registry = UnapprovedProcessPlugin {
      plugin: "npm:@dprint/exec@0.5.0".to_string(),
      registry: Some("https://npm.example.com/".to_string()),
      approval: None,
    };
    let trusted_config_map = ConfigMap::from([("lineWidth".to_string(), ConfigMapValue::from_i32(80))]);
    let parts = [plugin(Some(Some("1:hash"))), plugin(Some(None)), plugin(None), from_registry]
      .iter()
      .map(get_process_plugin_id_part)
      .chain([get_trusted_config_map_hash(&trusted_config_map)])
      .collect::<Vec<_>>();
    assert_eq!(
      parts,
      vec![
        "https://plugins.dprint.dev/exec-0.5.0.json@checksum\n1:hash",
        "https://plugins.dprint.dev/exec-0.5.0.json@checksum\nanyConfig",
        "https://plugins.dprint.dev/exec-0.5.0.json@checksum\nnotSetUp",
        "npm:@dprint/exec@0.5.0\nnotSetUp\nregistry: https://npm.example.com/",
        "1:2fa6774bc8dbe963c51fb07f6539189e5d272df1e4b048319a3cd9b093e6fd48",
      ]
    );
    assert_eq!(
      get_approval_id("processPlugins", "file:///a/dprint.json", &parts),
      "d38ae5893a98161365bed9043499c95b7e4e3949aaa0d3151c94e9cc9a985743"
    );
    assert_eq!(
      get_approval_id("download", "file:///a/dprint.json", &["https://example.com/plugin.wasm".to_string()]),
      "2ab4d0c7e0fbd9986542b36b33f2be90fd9166a85487acdfbb050927c923c5d5"
    );
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
    let items = get_download_items(&[url.clone(), proxy.to_download()], ApprovalItemKind::Plugin, &editor_paths);
    assert_eq!(
      items,
      vec![
        ApprovalItem {
          specifier: url,
          kind: ApprovalItemKind::Plugin,
          registry: None,
          npmrc_uri: None,
          plugin: None,
          configuration: None,
        },
        ApprovalItem {
          specifier: "https://${TOKEN}@proxy.example".to_string(),
          kind: ApprovalItemKind::NpmrcProxy,
          registry: None,
          npmrc_uri: Some("file:///link/project/.npmrc".parse().unwrap()),
          plugin: None,
          configuration: None,
        },
      ]
    );
    assert_eq!(get_download_specifier(&proxy.to_download()), "https://${TOKEN}@proxy.example");
  }
}
