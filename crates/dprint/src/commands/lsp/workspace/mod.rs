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
use crate::configuration::DOT_CONFIG_DIR_NAME;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::SessionPermissions;
use crate::configuration::resolve_global_config_path_and_text;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::paths::get_plugin_names_for_file_on_disk;
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
use crate::utils::PathSource;
use crate::utils::Semaphore;
use crate::utils::UntrustedDownloadError;
use crate::utils::get_bytes_hash;
use crate::utils::get_untrusted_download_message;

mod config_diagnostics;
pub mod messages;

use self::config_diagnostics::ConfigDiagnostic;
use self::config_diagnostics::PluginSectionDiagnostic;
use self::config_diagnostics::get_config_diagnostics;
use self::config_diagnostics::get_config_error_diagnostic;
use self::messages::ApprovalDecision;
use self::messages::ApprovalItem;
use self::messages::ApprovalItemKind;
use self::messages::ApprovalKind;
use self::messages::ConfigChangeReason;
use self::messages::ConfigFileInfo;
use self::messages::ConfigFileKind;
use self::messages::ConfigInfoResult;
use self::messages::DocumentLanguage;
use self::messages::FileInfoResult;
use self::messages::FilePlugin;
use self::messages::FormatFailedReason;
use self::messages::FormatStatus;
use self::messages::GlobalConfigUse;
use self::messages::NotFormatted;
use self::messages::NotFormattedReason;
use self::messages::PendingApproval;
use self::messages::PluginInfo;

/// The error for approving what isn't pending anymore (ex. what needs to be
/// approved changed because the config file changed).
#[derive(Debug, thiserror::Error)]
#[error("Nothing needs to be approved with id {id}, which may be because what needs to be approved changed.")]
pub struct ApprovalChangedError {
  id: String,
  /// What needs to be approved for the config file now when it's known.
  pub approvals: Option<Vec<PendingApproval>>,
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
    Ok(err) => Ok(NotFormatted {
      reason: err.reason,
      message: Some(err.message),
      approvals: Vec::new(),
    }),
    Err(err) => Err(err),
  }
}

/// The error for params that can't be handled (ex. a document that isn't a
/// file or a decision that an approval doesn't have).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidParamsError(String);

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
  published_diagnostics: RefCell<HashMap<String, Vec<ConfigDiagnostic>>>,
  /// The plugins of each config file uri the last time they were known, which
  /// are provided when it can't be used (ex. it has an error) so that the
  /// editor doesn't change what it formats with it.
  last_plugin_infos: RefCell<HashMap<String, Vec<PluginInfo>>>,
  /// What needs to be approved for each config file, which is what a client
  /// approves by its id.
  approvals: RefCell<HashMap<String, Vec<StoredApproval>>>,
  /// A file that each approval that was provided is for, which says what
  /// needs to be approved now for an approval that isn't pending anymore.
  approval_files: RefCell<HashMap<String, (PathBuf, bool)>>,
  /// The file patterns of each config file uri the last time they were known
  /// (see `last_plugin_infos`).
  last_file_patterns: RefCell<HashMap<String, Vec<String>>>,
  /// The ids of the downloads the user denied this session.
  denied_downloads: RefCell<HashSet<String>>,
  /// The ids of the approvals that a language client's user was asked about
  /// this session.
  prompted_approvals: RefCell<HashSet<String>>,
  /// Whether a language client's user was notified this session about a file
  /// without a config file.
  has_notified_no_config: Cell<bool>,
  /// The files the editor said changed that weren't handled yet.
  pending_config_changes: RefCell<Vec<WatchedFileChange>>,
}

/// What a config file resolved to.
#[derive(Clone, PartialEq)]
struct ResolvedConfigState {
  config: Rc<ResolvedConfig>,
  untrusted_downloads: Vec<String>,
  process_plugins_allowed: bool,
}

/// What needs to be approved for a config file.
#[derive(Clone)]
struct StoredApproval {
  approval: PendingApproval,
  subject: ApprovalSubject,
  /// A file the config file is used for, which is resolved again once it's
  /// approved.
  file_path: PathBuf,
  use_global_config: bool,
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
      last_file_patterns: Default::default(),
      denied_downloads: Default::default(),
      prompted_approvals: Default::default(),
      has_notified_no_config: Default::default(),
      pending_config_changes: Default::default(),
      environment,
      client,
      scope_container,
      concurrency_limiter,
      session_permissions,
    }
  }

  pub fn set_client_kind(&self, client_kind: ClientKind) {
    self.client_kind.set(client_kind);
  }

  pub fn set_settings(&self, settings: WorkspaceSettings) {
    self.environment.set_log_level(match settings.verbose {
      Some(true) => LogLevel::Debug,
      Some(false) if self.initial_log_level == LogLevel::Debug => LogLevel::Info,
      _ => self.initial_log_level,
    });
    *self.settings.borrow_mut() = settings;
  }

  /// Forgets the config files and stops the plugins, which are set up again
  /// when they're next used (ex. a plugin that stopped responding or changed).
  /// What the user decided for the session is kept.
  pub async fn reset(&self) {
    log_debug!(self.environment, "Resetting the configuration files and plugins.");
    self.scope_container.shutdown().await;
  }

  pub async fn shutdown(&self) {
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
        if change.is_config_file {
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
    let Some(uri) = file_path_to_uri(file_path) else {
      return;
    };
    match change {
      ConfigFileChange::Changed if self.was_resolved(file_path) => {
        let Some(dir_path) = get_config_file_dir(file_path) else {
          return;
        };
        // a file path in the directory has the config file be found for it, and
        // an editor extension is told when what it resolves to changed
        let _ = self.resolve_scope(&dir_path.join("file"), false).await;
      }
      ConfigFileChange::Deleted => {
        // a deleted config file doesn't have problems anymore
        self.publish_diagnostics(uri.clone(), Vec::new());
        self.send_config_changed(uri, ConfigChangeReason::Deleted);
      }
      ConfigFileChange::Created => self.send_config_changed(uri, ConfigChangeReason::Created),
      ConfigFileChange::Changed => self.send_config_changed(uri, ConfigChangeReason::Changed),
    }
  }

  /// Gets the config files that were resolved from the file (ex. it's a config
  /// file they extend).
  fn get_dependent_config_files(&self, file_path: &Path) -> Vec<PathBuf> {
    let file_path = self.canonicalize(file_path.to_path_buf());
    let mut config_file_paths = Vec::new();
    for state in self.resolved_configs.borrow().values() {
      if state.config.local_dependencies.contains(&file_path)
        && let Some(config_file_path) = state.config.source.maybe_local_path()
      {
        let config_file_path = config_file_path.as_ref().to_path_buf();
        if !config_file_paths.contains(&config_file_path) {
          config_file_paths.push(config_file_path);
        }
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
    let scope = match self.resolve_scope(&file_path, use_global_config).await? {
      DocumentConfig::Resolved(scope) => scope,
      DocumentConfig::NotFound => {
        let reason = self.get_no_config_reason(use_global_config);
        return Ok(FileInfoResult {
          config_file: None,
          plugins: Vec::new(),
          status: FormatStatus::NotFormatted(NotFormatted {
            reason,
            message: self.get_not_formatted_message(reason, is_notebook_cell, use_global_config),
            approvals: Vec::new(),
          }),
        });
      }
      DocumentConfig::Failed(failure) => {
        return Ok(FileInfoResult {
          plugins: Vec::new(),
          status: FormatStatus::NotFormatted(NotFormatted {
            reason: failure.reason,
            approvals: self.get_failure_approvals(&failure, &file_path, use_global_config),
            message: Some(failure.message),
          }),
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
    let approvals = self.get_scope_approvals(&scope, &file_path, use_global_config);
    Ok(FileInfoResult {
      config_file: self.get_config_file_info(&scope),
      plugins,
      status: match reason {
        Some(reason) => FormatStatus::NotFormatted(NotFormatted {
          reason,
          message: self.get_reason_message(&scope, reason, is_notebook_cell, use_global_config),
          approvals,
        }),
        None => FormatStatus::Ready,
      },
    })
  }

  pub async fn config_info(self: &Rc<Self>, uri: &Uri, global_config: Option<GlobalConfigUse>) -> Result<ConfigInfoResult> {
    let use_global_config = self.resolve_global_config_use(global_config);
    let config = match self.get_uri_file_path(uri) {
      Ok(file_path) => self.resolve_scope(&file_path, use_global_config).await.map(|config| (file_path, config)),
      Err(err) => Err(err),
    };
    let (file_path, config) = match config {
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
        let reason = self.get_no_config_reason(use_global_config);
        return Ok(ConfigInfoResult {
          status: FormatStatus::NotFormatted(NotFormatted {
            reason,
            message: self.get_not_formatted_message(reason, false, use_global_config),
            approvals: Vec::new(),
          }),
          ..Default::default()
        });
      }
      DocumentConfig::Failed(failure) => {
        let config_uri = failure.config_file.as_ref().map(|config_file| config_file.uri.as_str().to_string());
        let plugins = config_uri.as_ref().and_then(|uri| self.last_plugin_infos.borrow().get(uri).cloned());
        let file_patterns = config_uri
          .as_ref()
          .and_then(|uri| self.last_file_patterns.borrow().get(uri).cloned())
          .unwrap_or_default();
        return Ok(ConfigInfoResult {
          file_patterns,
          status: FormatStatus::NotFormatted(NotFormatted {
            reason: failure.reason,
            approvals: self.get_failure_approvals(&failure, &file_path, use_global_config),
            message: Some(failure.message),
          }),
          plugins,
          config_file: failure.config_file,
          ..Default::default()
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
    let formats_other_files = config.is_some_and(|config| {
      config.shebangs.as_ref().is_some_and(|shebangs| !shebangs.is_empty())
        // a plugin that isn't resolved has its associations in the config file
        || config
          .config_map
          .values()
          .any(|value| matches!(value, ConfigMapValue::PluginConfig(plugin_config) if plugin_config.associations.is_some()))
    });
    let file_patterns = match (config, &plugins) {
      (Some(config), Some(plugins)) => {
        let file_patterns = get_file_patterns(config, plugins);
        self.last_file_patterns.borrow_mut().insert(get_config_key(config), file_patterns.clone());
        file_patterns
      }
      _ => Vec::new(),
    };
    let watch_uris = config
      .map(|config| config.local_dependencies.iter().filter_map(|path| file_path_to_uri(path)).collect())
      .unwrap_or_default();
    let approvals = self.get_scope_approvals(&scope, &file_path, use_global_config);
    Ok(ConfigInfoResult {
      config_file: self.get_config_file_info(&scope),
      plugins,
      formats_other_files,
      file_patterns,
      watch_uris,
      status: match reason {
        Some(reason) => FormatStatus::NotFormatted(NotFormatted {
          reason,
          message: self.get_reason_message(&scope, reason, false, use_global_config),
          approvals,
        }),
        None => FormatStatus::Ready,
      },
    })
  }

  /// Gets the plugins of the config file from what they matched the last time
  /// it was resolved, which is `None` when that isn't known for each of them.
  fn get_last_plugin_infos(&self, config: &ResolvedConfig) -> Option<Vec<PluginInfo>> {
    if let Some(plugins) = self.last_plugin_infos.borrow().get(&get_config_key(config)) {
      return Some(plugins.clone());
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
    self.last_plugin_infos.borrow_mut().insert(get_config_key(config), plugins.to_vec());
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
  /// changed once the schemas that didn't download in time did.
  pub async fn config_schema(self: &Rc<Self>, uri: &Uri, global_config: Option<GlobalConfigUse>) -> (serde_json::Value, bool) {
    let mut plugins = Vec::new();
    let use_global_config = self.resolve_global_config_use(global_config);
    // a config file that fails to resolve was shown to the user, so its plugins are only left out
    let mut is_resolved = false;
    if let Ok(file_path) = self.get_uri_file_path(uri)
      && let Ok(DocumentConfig::Resolved(scope)) = self.resolve_scope(&file_path, use_global_config).await
    {
      // the plugins aren't known while they need to be approved
      is_resolved = scope.untrusted_downloads.is_empty() && scope.unapproved_process_plugins.is_none();
      plugins.extend(scope.plugins.values().map(|plugin| PluginSchemaUrl {
        config_key: plugin.info().config_key.clone(),
        url: plugin.info().config_schema_url.clone(),
      }));
    }
    let schema = self.config_schema_builder.build(&plugins).await;
    if schema.is_downloading {
      let workspace = self.clone();
      let uri = uri.clone();
      dprint_core::async_runtime::spawn(async move {
        let urls = plugins.iter().map(|plugin| plugin.url.as_str()).collect::<Vec<_>>();
        workspace.config_schema_builder.wait_for_downloads(&urls).await;
        workspace.send_config_changed(uri, ConfigChangeReason::Changed);
      });
    }
    (schema.schema, is_resolved && schema.is_complete)
  }

  /// Gets what needs to be approved for the config files that were used.
  pub fn pending_approvals(&self) -> Vec<PendingApproval> {
    self.approvals.borrow().values().flatten().map(|stored| stored.approval.clone()).collect()
  }

  pub async fn format(self: &Rc<Self>, request: FormatRequest, token: &Arc<CancellationToken>) -> Result<FormatOutcome> {
    let document_file = match self.get_document_file(&request.document) {
      Ok(document_file) => document_file,
      Err(err) => return Ok(FormatOutcome::NotFormatted(into_unsupported_document(err)?)),
    };
    let use_global_config = self.resolve_global_config_use(request.global_config);
    let is_notebook_cell = document_file.notebook_path.is_some();
    let file_path = document_file.candidates[0].clone();
    let scope = match self.resolve_scope(&file_path, use_global_config).await? {
      DocumentConfig::Resolved(scope) => scope,
      DocumentConfig::NotFound => {
        log_stderr_info!(self.environment, "Path did not have a dprint config file: {}", file_path.display());
        let reason = self.get_no_config_reason(use_global_config);
        if request.notify_no_config {
          self.notify_no_config(reason, use_global_config);
        }
        return Ok(FormatOutcome::NotFormatted(NotFormatted {
          reason,
          message: self.get_not_formatted_message(reason, is_notebook_cell, use_global_config),
          approvals: Vec::new(),
        }));
      }
      DocumentConfig::Failed(failure) => {
        let approvals = self.get_failure_approvals(&failure, &file_path, use_global_config);
        self.prompt_for_approvals(&approvals);
        return Ok(FormatOutcome::NotFormatted(NotFormatted {
          reason: failure.reason,
          message: Some(failure.message),
          approvals,
        }));
      }
    };
    // only asked when formatting because the other requests are made without
    // the user doing anything (ex. when a project is opened)
    let approvals = self.get_scope_approvals(&scope, &file_path, use_global_config);
    self.prompt_for_approvals(&approvals);
    let chosen_file = self.choose_file(&scope, document_file, Some(request.text.as_bytes()));
    let not_formatted = |reason| {
      Ok(FormatOutcome::NotFormatted(NotFormatted {
        reason,
        message: self.get_reason_message(&scope, reason, is_notebook_cell, use_global_config),
        approvals: approvals.clone(),
      }))
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
  pub async fn approve(self: &Rc<Self>, id: &str, decision: ApprovalDecision) -> Result<Vec<PendingApproval>> {
    let stored = self.approvals.borrow().values().flatten().find(|stored| stored.approval.id == id).cloned();
    let Some(stored) = stored else {
      // say what needs to be approved now when the config file is known
      let approval_file = self.approval_files.borrow().get(id).cloned();
      let approvals = match approval_file {
        Some((file_path, use_global_config)) => match self.resolve_scope(&file_path, use_global_config).await {
          Ok(DocumentConfig::Resolved(scope)) => Some(self.get_scope_approvals(&scope, &file_path, use_global_config)),
          Ok(DocumentConfig::Failed(failure)) => Some(self.get_failure_approvals(&failure, &file_path, use_global_config)),
          Ok(DocumentConfig::NotFound) | Err(_) => None,
        },
        None => None,
      };
      return Err(ApprovalChangedError { id: id.to_string(), approvals }.into());
    };
    if !stored.approval.decisions.contains(&decision) {
      return Err(InvalidParamsError(format!("The decision {:?} isn't one of the approval's decisions.", decision)).into());
    }
    match &stored.subject {
      ApprovalSubject::Downloads(urls) => {
        if decision.is_allowed() {
          self.environment.download_policy().allow(urls);
        } else {
          self.denied_downloads.borrow_mut().insert(stored.approval.id.clone());
        }
      }
      ApprovalSubject::ProcessPlugins(unapproved) => {
        let title = get_process_plugin_action_title(decision);
        if let Err(err) = store_process_plugin_action(&self.environment, &self.session_permissions, unapproved, Some(title)).await {
          let message = format!("Failed storing the decision for the process plugins: {:#}", err);
          log_warn!(self.environment, "{}", message);
          self.client.show_message(MessageType::WARNING, message);
        }
      }
    }
    // this downloads and sets up what was allowed, and tells the editor that
    // the config file formats files now
    let file_path = &stored.file_path;
    let use_global_config = stored.use_global_config;
    let config = self.resolve_scope(file_path, use_global_config).await;
    if let ApprovalSubject::ProcessPlugins(unapproved) = &stored.subject {
      // allowing a plugin to be set up is only for this, so a plugin that
      // wasn't set up (ex. it failed to) needs to be approved again
      for plugin in &unapproved.plugins {
        self
          .session_permissions
          .take_process_plugin_set_up_grant(&unapproved.config_file, &plugin.plugin);
      }
    }
    Ok(match config? {
      DocumentConfig::Resolved(scope) => self.get_scope_approvals(&scope, file_path, use_global_config),
      DocumentConfig::Failed(failure) => self.get_failure_approvals(&failure, file_path, use_global_config),
      DocumentConfig::NotFound => Vec::new(),
    })
  }

  /// Gets what needs to be approved for the scope to format files.
  fn get_scope_approvals(&self, scope: &PluginsScope<TEnvironment>, file_path: &Path, use_global_config: bool) -> Vec<PendingApproval> {
    let Some(config) = scope.config.as_ref() else {
      return Vec::new();
    };
    let mut subjects = Vec::new();
    if !scope.untrusted_downloads.is_empty() {
      let urls = scope.untrusted_downloads.clone();
      let items = get_download_items(&urls, ApprovalItemKind::Plugin);
      subjects.push((ApprovalSubject::Downloads(urls), items));
    }
    if let Some(unapproved) = &scope.unapproved_process_plugins {
      let items = unapproved.plugins.iter().map(|plugin| self.get_process_plugin_item(config, plugin)).collect();
      subjects.push((ApprovalSubject::ProcessPlugins(unapproved.clone()), items));
    }
    let config_file = path_source_to_uri(&config.source);
    self.store_approvals(get_config_key(config), config_file, subjects, file_path, use_global_config)
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
      kind: Some(ApprovalItemKind::Plugin),
      configuration,
      name: info.as_ref().map(|info| info.name.clone()),
      version: info.as_ref().map(|info| info.version.clone()),
      config_key: info.map(|info| info.config_key),
    }
  }

  /// Gets what needs to be approved for the config file to resolve.
  fn get_failure_approvals(&self, failure: &ConfigFailure, file_path: &Path, use_global_config: bool) -> Vec<PendingApproval> {
    let config_file = failure.config_file.as_ref().map(|config_file| config_file.uri.clone());
    let key = config_file.as_ref().map(|uri| uri.as_str().to_string()).unwrap_or_default();
    let mut subjects = Vec::new();
    if !failure.downloads.is_empty() {
      // what fails to download while resolving a config file is what it extends
      let urls = failure.downloads.clone();
      let items = get_download_items(&urls, ApprovalItemKind::Config);
      subjects.push((ApprovalSubject::Downloads(urls), items));
    }
    self.store_approvals(key, config_file, subjects, file_path, use_global_config)
  }

  /// Stores what needs to be approved for the config file, which replaces what
  /// was stored for it before.
  fn store_approvals(
    &self,
    config_key: String,
    config_file: Option<Uri>,
    subjects: Vec<(ApprovalSubject, Vec<ApprovalItem>)>,
    file_path: &Path,
    use_global_config: bool,
  ) -> Vec<PendingApproval> {
    let permissions = self.session_permissions.read(&self.environment);
    let stored = subjects
      .into_iter()
      .map(|(subject, items)| {
        let approval = match &subject {
          ApprovalSubject::Downloads(urls) => {
            let id = get_approval_id("download", &config_key, urls);
            let denied = self.denied_downloads.borrow().contains(&id);
            PendingApproval {
              denied,
              // a download is only denied for the session
              previous_decision: denied.then_some(ApprovalDecision::Deny),
              id,
              kind: ApprovalKind::Download,
              config_file: config_file.clone(),
              items,
              // a download is only allowed for the session
              decisions: vec![ApprovalDecision::Allow, ApprovalDecision::Deny],
              message: get_untrusted_download_message(urls),
            }
          }
          ApprovalSubject::ProcessPlugins(unapproved) => {
            // the configuration of each plugin is part of what's approved
            let parts = unapproved
              .plugins
              .iter()
              .map(|plugin| format!("{}\n{:?}", plugin.plugin, plugin.approval.as_ref().map(|approval| &approval.config_hash)))
              .collect::<Vec<_>>();
            PendingApproval {
              id: get_approval_id("processPlugins", &config_key, &parts),
              kind: ApprovalKind::ProcessPlugins,
              config_file: config_file.clone(),
              items,
              decisions: vec![
                ApprovalDecision::Allow,
                ApprovalDecision::AlwaysAllow,
                ApprovalDecision::Deny,
                ApprovalDecision::AlwaysDeny,
              ],
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
          approval,
          subject,
          file_path: file_path.to_path_buf(),
          use_global_config,
        }
      })
      .collect::<Vec<_>>();
    let approvals = stored.iter().map(|stored| stored.approval.clone()).collect();
    for stored in &stored {
      self
        .approval_files
        .borrow_mut()
        .insert(stored.approval.id.clone(), (stored.file_path.clone(), stored.use_global_config));
    }
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
          ApprovalKind::Download => {
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
          ApprovalKind::Download => vec![DOWNLOAD_ACTION_TITLE, CANCEL_DOWNLOAD_ACTION_TITLE],
          ApprovalKind::ProcessPlugins => PROCESS_PLUGIN_ACTION_TITLES.to_vec(),
        };
        let Some(title) = workspace.show_message_request(MessageType::WARNING, approval.message.clone(), &titles).await else {
          // not choosing an action leaves it undecided
          return;
        };
        let decision = match approval.kind {
          ApprovalKind::Download if title == DOWNLOAD_ACTION_TITLE => ApprovalDecision::Allow,
          ApprovalKind::Download => ApprovalDecision::Deny,
          ApprovalKind::ProcessPlugins => match get_process_plugin_decision(&title) {
            Some(decision) => decision,
            // the client responded with an action that wasn't offered
            None => return,
          },
        };
        match workspace.approve(&approval.id, decision).await {
          Ok(approvals) => pending.extend(workspace.take_unprompted_approvals(approvals.into_iter())),
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
    FormatOutcome::NotFormatted(NotFormatted {
      reason: NotFormattedReason::ConfigDiagnostics,
      message: Some(message),
      approvals: Vec::new(),
    })
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
          reason: NotFormattedReason::ConfigError,
          message: format!("{:#}", err),
          downloads: Vec::new(),
        }));
      }
    };
    let Some(config_file) = config_file else {
      return Ok(DocumentConfig::NotFound);
    };
    let key = get_config_file_key(&config_file);
    let failed = |reason: NotFormattedReason, message: String, downloads: Vec<String>| {
      DocumentConfig::Failed(ConfigFailure {
        config_file: path_source_to_uri(&config_file.source).map(|uri| ConfigFileInfo {
          uri,
          dir_uri: get_config_dir_uri(&config_file.source, config_file.is_global_config, config_file.base_path.as_ref()),
          kind: self.get_config_file_kind(config_file.is_global_config),
        }),
        reason,
        message,
        downloads,
      })
    };
    let scope = match self.scope_container.resolve_config_file(&config_file).await {
      Ok(scope) => scope,
      Err(err) => {
        // what it formats changed when it resolved before
        if self.resolved_configs.borrow_mut().remove(&key).is_some()
          && let Some(uri) = path_source_to_uri(&config_file.source)
        {
          self.send_config_changed(uri, ConfigChangeReason::Changed);
        }
        // not an error because it resolves once the user allows downloading what it extends
        if let Some(err) = UntrustedDownloadError::find(&err) {
          let downloads = vec![err.url.clone()];
          return Ok(failed(
            NotFormattedReason::ApprovalRequired,
            get_untrusted_download_message(&downloads),
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
      self.show_config_error(key, &err);
      return Ok(failed(NotFormattedReason::ConfigError, format!("{:#}", err), Vec::new()));
    }
    self.config_errors.borrow_mut().remove(&key);
    self.notify_if_config_changed(key, &config_file, &scope);
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
    if let Some(uri) = path_source_to_uri(source) {
      self.publish_diagnostics(uri, diagnostics);
    }
  }

  fn publish_diagnostics(&self, uri: Uri, diagnostics: Vec<ConfigDiagnostic>) {
    let mut published_diagnostics = self.published_diagnostics.borrow_mut();
    let previous = published_diagnostics.get(uri.as_str());
    // there's nothing to clear when nothing was published
    if previous == Some(&diagnostics) || previous.is_none() && diagnostics.is_empty() {
      return;
    }
    published_diagnostics.insert(uri.as_str().to_string(), diagnostics.clone());
    drop(published_diagnostics);
    self
      .client
      .publish_diagnostics(uri, diagnostics.into_iter().map(|diagnostic| diagnostic.into_lsp_diagnostic()).collect());
  }

  /// Tells an editor extension when the config file resolves to a different
  /// config than before because what it formats may have changed in a way the
  /// editor can't see (ex. a config file that it extends changed or what
  /// needed to be approved was approved).
  fn notify_if_config_changed(&self, key: String, config_file: &ResolvedConfigPathWithText, scope: &PluginsScope<TEnvironment>) {
    let Some(config) = &scope.config else {
      return;
    };
    let state = ResolvedConfigState {
      config: config.clone(),
      untrusted_downloads: scope.untrusted_downloads.clone(),
      process_plugins_allowed: scope.unapproved_process_plugins.is_none(),
    };
    let previous = self.resolved_configs.borrow_mut().insert(key, state.clone());
    if previous.is_some_and(|previous| previous != state)
      && let Some(uri) = path_source_to_uri(&config_file.source)
    {
      self.send_config_changed(uri, ConfigChangeReason::Changed);
    }
  }

  /// Tells an editor extension that what a config file formats may have changed.
  fn send_config_changed(&self, uri: Uri, reason: ConfigChangeReason) {
    if self.client_kind.get() == ClientKind::EditorExtension {
      self.client.send_config_changed(uri, reason);
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
    Some(ConfigFileInfo {
      uri: path_source_to_uri(&config.source)?,
      dir_uri: get_config_dir_uri(&config.source, config.is_global, config.base_path.as_ref()),
      kind: self.get_config_file_kind(config.is_global),
    })
  }

  fn get_config_file_kind(&self, is_global: bool) -> ConfigFileKind {
    if self.scope_container.has_config_override() {
      ConfigFileKind::Override
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

  /// Gets the message for why the scope doesn't format a file.
  fn get_reason_message(
    &self,
    scope: &PluginsScope<TEnvironment>,
    reason: NotFormattedReason,
    is_notebook_cell: bool,
    use_global_config: bool,
  ) -> Option<String> {
    match (reason, &scope.unapproved_process_plugins) {
      (NotFormattedReason::ApprovalRequired, _) if !scope.untrusted_downloads.is_empty() => Some(get_untrusted_download_message(&scope.untrusted_downloads)),
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
      NotFormattedReason::UnsupportedDocument | NotFormattedReason::LanguageRequired => None,
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
      return Some(NotFormattedReason::ApprovalRequired);
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

  /// Gets the files the document may be formatted as.
  fn get_document_file(&self, document: &DocumentInfo) -> Result<DocumentFile> {
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
    self.environment.canonicalize_maybe_not_exists(&path).map(|p| p.into_path_buf()).unwrap_or(path)
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
    supports_range_formatting: info.supports_range_formatting,
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
fn get_file_patterns(config: &ResolvedConfig, plugins: &[PluginInfo]) -> Vec<String> {
  let mut patterns = Vec::new();
  let mut add = |pattern: String| {
    if !patterns.contains(&pattern) {
      patterns.push(pattern);
    }
  };
  for plugin in plugins {
    for extension in &plugin.file_extensions {
      add(format!("**/*.{}", to_case_insensitive_glob(extension)));
    }
    for file_name in &plugin.file_names {
      add(format!("**/{}", to_case_insensitive_glob(file_name)));
    }
    if let Some(ConfigMapValue::PluginConfig(plugin_config)) = config.config_map.get(&plugin.config_key) {
      for association in plugin_config.associations.iter().flatten() {
        // an association that excludes files is in the includes and excludes, which aren't in the patterns
        if !association.starts_with('!') {
          add(association.clone());
        }
      }
    }
  }
  patterns
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
    } else if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '!' | ',') {
      // a character the glob would otherwise treat specially
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
fn get_config_dir_uri(source: &PathSource, is_global: bool, base_path: &Path) -> Option<Uri> {
  if is_global {
    // the global config file is used for each root directory (ex. drive)
    return file_path_to_uri(base_path);
  }
  match source {
    PathSource::Local(local) => file_path_to_uri(get_config_file_dir(local.path.as_ref())?),
    PathSource::Remote(_) | PathSource::Npm(_) => None,
  }
}

/// Gets the directory a local config file is used for, which is the
/// directory it's in unless that's a `.config` directory.
fn get_config_file_dir(file_path: &Path) -> Option<&Path> {
  let dir_path = file_path.parent()?;
  Some(match dir_path.file_name() {
    Some(name) if name == DOT_CONFIG_DIR_NAME => dir_path.parent().unwrap_or(dir_path),
    _ => dir_path,
  })
}

/// Gets the key of what's approved for the config.
fn get_config_key(config: &ResolvedConfig) -> String {
  path_source_to_uri(&config.source)
    .map(|uri| uri.as_str().to_string())
    .unwrap_or_else(|| config.source.display())
}

/// Gets the items of urls or paths to download.
fn get_download_items(urls: &[String], kind: ApprovalItemKind) -> Vec<ApprovalItem> {
  urls
    .iter()
    .map(|url| ApprovalItem {
      specifier: url.clone(),
      kind: Some(kind),
      name: None,
      version: None,
      config_key: None,
      configuration: None,
    })
    .collect()
}

/// Gets an id that changes when what's approved does.
fn get_approval_id(kind: &str, config_key: &str, items: &[String]) -> String {
  let mut text = format!("{}\n{}", kind, config_key);
  for item in items {
    text.push('\n');
    text.push_str(item);
  }
  format!("{:x}", get_bytes_hash(text.as_bytes()))
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
  let extension_file_names = language
    .file_extensions
    .iter()
    .filter(|extension| extension.len() > 1 && extension.starts_with('.'))
    .map(|extension| format!("{}{}", base_name, extension));
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
