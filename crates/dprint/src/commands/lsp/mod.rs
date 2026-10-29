use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range as StdRange;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::CancellationToken;
use deno_tower_lsp::LanguageServer;
use deno_tower_lsp::LspService;
use deno_tower_lsp::Server;
use deno_tower_lsp::jsonrpc::Error as LspError;
use deno_tower_lsp::jsonrpc::ErrorCode;
use deno_tower_lsp::jsonrpc::Result as LspResult;
use deno_tower_lsp::lsp_types::ClientCapabilities;
use deno_tower_lsp::lsp_types::CompletionList;
use deno_tower_lsp::lsp_types::CompletionParams;
use deno_tower_lsp::lsp_types::CompletionResponse;
use deno_tower_lsp::lsp_types::DidChangeConfigurationParams;
use deno_tower_lsp::lsp_types::DidChangeNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidChangeTextDocumentParams;
use deno_tower_lsp::lsp_types::DidChangeWatchedFilesParams;
use deno_tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions;
use deno_tower_lsp::lsp_types::DidChangeWorkspaceFoldersParams;
use deno_tower_lsp::lsp_types::DidCloseNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidCloseTextDocumentParams;
use deno_tower_lsp::lsp_types::DidOpenNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidOpenTextDocumentParams;
use deno_tower_lsp::lsp_types::DocumentChanges;
use deno_tower_lsp::lsp_types::DocumentFormattingParams;
use deno_tower_lsp::lsp_types::DocumentRangeFormattingParams;
use deno_tower_lsp::lsp_types::ExecuteCommandOptions;
use deno_tower_lsp::lsp_types::ExecuteCommandParams;
use deno_tower_lsp::lsp_types::FileChangeType;
use deno_tower_lsp::lsp_types::FileSystemWatcher;
use deno_tower_lsp::lsp_types::FormattingOptions;
use deno_tower_lsp::lsp_types::FormattingProperty;
use deno_tower_lsp::lsp_types::GlobPattern;
use deno_tower_lsp::lsp_types::Hover;
use deno_tower_lsp::lsp_types::HoverParams;
use deno_tower_lsp::lsp_types::InitializeParams;
use deno_tower_lsp::lsp_types::InitializeResult;
use deno_tower_lsp::lsp_types::InitializedParams;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::NumberOrString;
use deno_tower_lsp::lsp_types::OneOf;
use deno_tower_lsp::lsp_types::OptionalVersionedTextDocumentIdentifier;
use deno_tower_lsp::lsp_types::Position;
use deno_tower_lsp::lsp_types::Range;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::ServerCapabilities;
use deno_tower_lsp::lsp_types::ServerInfo;
use deno_tower_lsp::lsp_types::TextDocumentEdit;
use deno_tower_lsp::lsp_types::TextDocumentSyncCapability;
use deno_tower_lsp::lsp_types::TextDocumentSyncKind;
use deno_tower_lsp::lsp_types::TextDocumentSyncOptions;
use deno_tower_lsp::lsp_types::TextEdit;
use deno_tower_lsp::lsp_types::Uri;
use deno_tower_lsp::lsp_types::WorkDoneProgress;
use deno_tower_lsp::lsp_types::WorkDoneProgressBegin;
use deno_tower_lsp::lsp_types::WorkDoneProgressEnd;
use deno_tower_lsp::lsp_types::WorkDoneProgressReport;
use deno_tower_lsp::lsp_types::WorkspaceEdit;
use deno_tower_lsp::lsp_types::WorkspaceFolder;
use deno_tower_lsp::lsp_types::WorkspaceFoldersServerCapabilities;
use deno_tower_lsp::lsp_types::WorkspaceServerCapabilities;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::JoinHandle;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::plugins::process::start_parent_process_checker_task;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::try_join;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::cache::RemoteCacheMode;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::configuration::SessionPermissions;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::utils::DownloadPolicy;
use crate::utils::LogLevel;
use crate::utils::OutputSink;
use crate::utils::Semaphore;

use self::client::ClientWrapper;
use self::config_completion::ConfigCompletions;
use self::config_completion::get_config_file_capabilities;
use self::config_completion::is_config_uri;
use self::documents::Documents;
use self::notebook::get_notebook_cell_format_registrations;
use self::notebook::get_notebook_document_sync_options;
use self::settings::LspSettings;
use self::text::LineIndex;
use self::text::to_lsp_edits;
use self::untitled::get_untitled_registrations;
use self::untitled::is_untitled_uri;
use self::workspace::ApprovalChangedError;
use self::workspace::ClientKind;
use self::workspace::ConfigFileChange;
use self::workspace::DocumentInfo;
use self::workspace::FormatOutcome;
use self::workspace::FormatRequest;
use self::workspace::UnsupportedDocumentError;
use self::workspace::Workspace;
use self::workspace::WorkspaceSettings;
use self::workspace::messages::APPROVE_METHOD;
use self::workspace::messages::ApproveParams;
use self::workspace::messages::ApproveResult;
use self::workspace::messages::CONFIG_INFO_METHOD;
use self::workspace::messages::CONFIG_SCHEMA_METHOD;
use self::workspace::messages::ConfigInfoResult;
use self::workspace::messages::ConfigParams;
use self::workspace::messages::ConfigSchemaResult;
use self::workspace::messages::DocumentLanguage;
use self::workspace::messages::DocumentParams;
use self::workspace::messages::EXPERIMENTAL_CAPABILITY_NAME;
use self::workspace::messages::EXPERIMENTAL_CAPABILITY_VERSION;
use self::workspace::messages::ExperimentalCapability;
use self::workspace::messages::FILE_INFO_METHOD;
use self::workspace::messages::FORMAT_METHOD;
use self::workspace::messages::FileInfoResult;
use self::workspace::messages::FormatParams;
use self::workspace::messages::FormatResult;
use self::workspace::messages::NotFormattedReason;
use self::workspace::messages::OVERRIDE_CONFIG_KEYS;
use self::workspace::messages::RESET_METHOD;

mod client;
pub(super) mod config_completion;
mod documents;
pub(super) mod language;
pub(super) mod notebook;
mod settings;
pub(super) mod text;
pub(super) mod untitled;
pub(super) mod workspace;

/// The formatting option a client provides in a format request to format a
/// document without a config file in an ancestor directory using the global
/// config file when the server isn't set to use the global config file.
const USE_GLOBAL_CONFIG_OPTION: &str = "useGlobalConfig";

/// The commands for formatting a document or a range of it using the global
/// config file when there's no config file in an ancestor directory, which
/// work regardless of whether the server is set to use the global config file.
const FORMAT_WITH_GLOBAL_CONFIG_COMMAND: &str = "dprint.formatWithGlobalConfig";
const FORMAT_SELECTION_WITH_GLOBAL_CONFIG_COMMAND: &str = "dprint.formatSelectionWithGlobalConfig";

/// How many requests and notifications are handled at the same time. A request
/// may wait a while (ex. for the plugins of a config file to be downloaded), so
/// this is high enough that it doesn't hold up the others (ex. a format on save
/// for another config file). Formatting is limited separately.
const MAX_CONCURRENT_MESSAGES: usize = 64;

/// The configuration files the client tells the server about when they change,
/// which includes the ones in a `.config` directory.
const CONFIG_FILE_GLOB: &str = "**/{dprint,.dprint}.{json,jsonc}";

// deno_tower_lsp will drop the future on cancellation,
// so use this to cancel the containing token on drop.
struct DropToken {
  completed: bool,
  token: Arc<tokio_util::sync::CancellationToken>,
}

impl DropToken {
  pub fn new(token: Arc<tokio_util::sync::CancellationToken>) -> Self {
    Self { token, completed: false }
  }

  pub fn completed(&mut self) {
    self.completed = true;
  }
}

impl Drop for DropToken {
  fn drop(&mut self) {
    if !self.completed {
      self.token.cancel();
    }
  }
}

/// What the `Workspace` does on the message handler's task.
type WorkspaceTask<TEnvironment> = Box<dyn FnOnce(Rc<Workspace<TEnvironment>>) -> LocalBoxFuture<'static, ()> + Send>;

struct ConfigEditorRequest {
  pub file_path: PathBuf,
  pub file_text: Arc<String>,
  pub position: Position,
  /// Whether to use the global config file when there's no config file in
  /// an ancestor directory.
  pub use_global_config: bool,
}

enum ChannelMessage<TEnvironment: Environment> {
  /// The client to send messages to, which is the first message.
  SetClient(ClientWrapper),
  Workspace(WorkspaceTask<TEnvironment>),
  Completion(ConfigEditorRequest, oneshot::Sender<Option<CompletionList>>),
  Hover(ConfigEditorRequest, oneshot::Sender<Option<Hover>>),
  Shutdown(oneshot::Sender<()>),
  /// This message is used for testing.
  #[cfg(test)]
  HasPending(oneshot::Sender<bool>),
}

pub async fn run_language_server<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> anyhow::Result<()> {
  let stdin = tokio::io::stdin();
  let stdout = tokio::io::stdout();
  let (tx, rx) = mpsc::unbounded_channel();

  let config_path = args.config.as_ref().map(|config| environment.cwd().join(config));
  let session_permissions = Arc::new(SessionPermissions::default());
  let recv_task = start_message_handler(
    environment,
    plugin_resolver,
    config_path,
    args.remote_cache_mode(),
    session_permissions.clone(),
    rx,
  );

  let environment = environment.clone();
  let lsp_task = dprint_core::async_runtime::spawn(async move {
    let (service, socket, pending) = with_dprint_methods(LspService::build(|client| {
      let client = ClientWrapper::new(Arc::new(client));
      Backend::new(client.clone(), environment.clone(), tx)
    }))
    .finish();
    Server::new(stdin, stdout, socket, pending)
      .concurrency_level(MAX_CONCURRENT_MESSAGES)
      .serve(service)
      .await;
  });

  let (shutdown_received, _) = try_join!(recv_task, lsp_task)?;

  // the plugins are only shut down on a shutdown request, so ensure that's
  // done before exiting when the client exited or disconnected without one
  plugin_resolver.clear_and_shutdown_initialized().await;

  // Exit the process instead of returning. The framework reads stdin on a
  // blocking task that stays parked in a read while the client keeps the pipe
  // open and dropping the tokio runtime waits for that task, so returning would
  // keep the process alive until the client closes stdin.
  std::process::exit(get_exit_code(shutdown_received))
}

/// Adds the `dprint/*` methods that an editor extension uses.
fn with_dprint_methods<TEnvironment: Environment>(
  builder: deno_tower_lsp::LspServiceBuilder<Backend<TEnvironment>>,
) -> deno_tower_lsp::LspServiceBuilder<Backend<TEnvironment>> {
  builder
    .custom_method(FORMAT_METHOD, Backend::dprint_format)
    .custom_method(FILE_INFO_METHOD, Backend::dprint_file_info)
    .custom_method(CONFIG_INFO_METHOD, Backend::dprint_config_info)
    .custom_method(CONFIG_SCHEMA_METHOD, Backend::dprint_config_schema)
    .custom_method(APPROVE_METHOD, Backend::dprint_approve)
    .custom_method(RESET_METHOD, Backend::dprint_reset)
}

/// Starts the task that handles the messages sent by the `Backend`, which
/// resolves to whether it stopped because of a shutdown request rather than
/// because the `Backend` was dropped.
fn start_message_handler<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  config_override: Option<PathBuf>,
  remote_cache_mode: RemoteCacheMode,
  session_permissions: Arc<SessionPermissions>,
  mut rx: mpsc::UnboundedReceiver<ChannelMessage<TEnvironment>>,
) -> JoinHandle<bool> {
  // IMPORTANT: Only what's on the trusted domains is downloaded without the
  // user allowing it because opening a project isn't the same as choosing to
  // download what its config file says (see `DownloadPolicy`).
  environment.set_download_policy(DownloadPolicy::for_editor(environment));
  // tower_lsp required Backend to implement Send and Sync, but
  // we use a single threaded runtime. So spawn some tasks and
  // communicate over a channel.
  // todo: deno_tower_lsp doesn't require this, so the channel could be removed
  let max_cores = environment.max_threads();
  let concurrency_limiter = Rc::new(Semaphore::new(std::cmp::max(1, max_cores - 1)));
  let environment = environment.clone();
  let scope_container = Rc::new(PluginsScopeContainer::new(
    environment.clone(),
    plugin_resolver.clone(),
    config_override,
    remote_cache_mode,
    session_permissions.clone(),
  ));
  let config_completions = Rc::new(ConfigCompletions::new(environment.clone(), scope_container.clone()));
  dprint_core::async_runtime::spawn(async move {
    let mut workspace = None;
    let pending_count = Rc::new(Cell::new(0usize));
    while let Some(message) = rx.recv().await {
      match message {
        ChannelMessage::SetClient(client) => {
          workspace = Some(Rc::new(Workspace::new(
            environment.clone(),
            client,
            scope_container.clone(),
            concurrency_limiter.clone(),
            session_permissions.clone(),
          )));
        }
        ChannelMessage::Workspace(task) => {
          let Some(workspace) = workspace.clone() else {
            continue;
          };
          pending_count.set(pending_count.get() + 1);
          let pending_count = pending_count.clone();
          dprint_core::async_runtime::spawn(async move {
            task(workspace).await;
            pending_count.set(pending_count.get() - 1);
          });
        }
        ChannelMessage::Completion(request, sender) => {
          let config_completions = config_completions.clone();
          dprint_core::async_runtime::spawn(async move {
            let result = config_completions
              .completions(&request.file_path, &request.file_text, request.position, request.use_global_config)
              .await;
            let _ = sender.send(result);
          });
        }
        ChannelMessage::Hover(request, sender) => {
          let config_completions = config_completions.clone();
          dprint_core::async_runtime::spawn(async move {
            let result = config_completions
              .hover(&request.file_path, &request.file_text, request.position, request.use_global_config)
              .await;
            let _ = sender.send(result);
          });
        }
        ChannelMessage::Shutdown(sender) => {
          match &workspace {
            Some(workspace) => workspace.shutdown().await,
            None => scope_container.shutdown().await,
          }
          let _ = sender.send(());
          return true; // exit
        }
        #[cfg(test)]
        ChannelMessage::HasPending(sender) => {
          let _ = sender.send(pending_count.get() > 0);
        }
      }
    }
    false
  })
}

/// Gets the exit code the language server protocol specifies for the server,
/// which is 0 when a shutdown request was received and 1 otherwise.
fn get_exit_code(shutdown_received: bool) -> i32 {
  if shutdown_received { 0 } else { 1 }
}

struct State<TEnvironment: Environment> {
  documents: Documents<TEnvironment>,
  /// Registrations to send to the client once it says it's initialized.
  pending_registrations: Vec<Registration>,
  /// The paths of the client's workspace folders on the file system.
  workspace_folders: Vec<PathBuf>,
  settings: LspSettings,
  client_kind: ClientKind,
  /// Whether the client supports the versioned document edits of a `WorkspaceEdit`.
  supports_document_changes: bool,
}

struct Backend<TEnvironment: Environment> {
  client: ClientWrapper,
  environment: TEnvironment,
  sender: mpsc::UnboundedSender<ChannelMessage<TEnvironment>>,
  state: Mutex<State<TEnvironment>>,
}

impl<TEnvironment: Environment> Backend<TEnvironment> {
  pub fn new(client: ClientWrapper, environment: TEnvironment, sender: mpsc::UnboundedSender<ChannelMessage<TEnvironment>>) -> Self {
    let _ = sender.send(ChannelMessage::SetClient(client.clone()));
    Backend {
      client,
      sender,
      state: Mutex::new(State {
        settings: LspSettings::from_environment(&environment),
        documents: Documents::new(environment.clone()),
        pending_registrations: Vec::new(),
        workspace_folders: Vec::new(),
        client_kind: ClientKind::LanguageClient {
          supports_message_actions: false,
        },
        supports_document_changes: false,
      }),
      environment,
    }
  }

  /// Runs the task with the `Workspace` on the message handler's task, which
  /// is `None` when the message handler stopped.
  async fn run<T: Send + 'static>(&self, task: impl FnOnce(Rc<Workspace<TEnvironment>>) -> LocalBoxFuture<'static, T> + Send + 'static) -> Option<T> {
    let (sender, receiver) = oneshot::channel();
    let task: WorkspaceTask<TEnvironment> = Box::new(move |workspace| {
      async move {
        let _ = sender.send(task(workspace).await);
      }
      .boxed_local()
    });
    self.sender.send(ChannelMessage::Workspace(task)).ok()?;
    receiver.await.ok()
  }

  /// Runs the task with the `Workspace` on the message handler's task without
  /// waiting on it.
  fn spawn(&self, task: impl FnOnce(Rc<Workspace<TEnvironment>>) -> LocalBoxFuture<'static, ()> + Send + 'static) {
    let _ = self.sender.send(ChannelMessage::Workspace(Box::new(task)));
  }

  /// Sends the settings to the `Workspace`.
  async fn update_workspace_settings(&self) {
    let settings = {
      let state = self.state.lock();
      WorkspaceSettings {
        use_global_config: state.settings.use_global_config,
        show_no_config_notification: state.settings.show_no_config_notification,
        ensure_stable_format: state.settings.ensure_stable_format,
        verbose: state.settings.verbose,
        workspace_folders: state.workspace_folders.clone(),
      }
    };
    self
      .run(move |workspace| {
        workspace.set_settings(settings);
        async {}.boxed_local()
      })
      .await;
  }

  /// Formats the document for a format request of a language client.
  async fn format_document(&self, uri: &Uri, range: Option<Range>, options: &FormattingOptions, token: CancellationToken) -> Option<Vec<TextEdit>> {
    let use_global_config = has_use_global_config_option(options);
    let (line_index, outcome) = self.format(uri, range, use_global_config, true, token).await?;
    match outcome {
      FormatOutcome::Formatted(edits) if edits.is_empty() => None,
      FormatOutcome::Formatted(edits) => Some(to_lsp_edits(edits, &line_index)),
      FormatOutcome::NotFormatted { reason, message, .. } => {
        match (reason, message) {
          // only logged because a client may format a range without the user asking (ex. on paste)
          (NotFormattedReason::RangeFormattingNotSupported, Some(message)) => self.client.log_info(message),
          // the problems are only shown once, so tell the client each time
          (NotFormattedReason::ConfigDiagnostics | NotFormattedReason::ConfigError, Some(message)) => self.log_format_failure(uri, &message),
          _ => {}
        }
        None
      }
      FormatOutcome::Failed { message, .. } => {
        self.log_format_failure(uri, &message);
        None
      }
    }
  }

  /// Formats the document or a range of it using the global config file when
  /// there's no config file in an ancestor directory and applies the edits.
  /// The user explicitly ran the command, so this always says why when the
  /// document wasn't formatted except for when it's already formatted.
  async fn format_with_global_config(&self, uri: &Uri, range: Option<Range>, token: CancellationToken) {
    let version = self.state.lock().documents.get_version(uri);
    let message = match self.format(uri, range, true, false, token).await {
      Some((line_index, FormatOutcome::Formatted(edits))) => {
        // the edits don't apply to the document anymore when it changed while formatting
        if edits.is_empty() || self.state.lock().documents.get_version(uri) != version {
          return;
        }
        let supports_document_changes = self.state.lock().supports_document_changes;
        let edits = to_lsp_edits(edits, &line_index);
        match self.client.apply_edit(new_workspace_edit(uri, version, edits, supports_document_changes)).await {
          Ok(()) => return,
          Err(err) => {
            let message = format!("Failed applying the edits for '{}': {:#}", uri.as_str(), err);
            log_error!(self.environment, "{}", message);
            self.client.log_error(message);
            None
          }
        }
      }
      Some((_, FormatOutcome::NotFormatted { message, .. })) => message.map(|message| (MessageType::INFO, message)),
      Some((_, FormatOutcome::Failed { message, .. })) => {
        self.log_format_failure(uri, &message);
        None
      }
      // why was logged
      None => None,
    };
    let (message_type, message) = message.unwrap_or_else(|| {
      (
        MessageType::WARNING,
        "dprint failed to format this document. See the language server's log for details.".to_string(),
      )
    });
    self.client.show_message(message_type, message);
  }

  /// Formats a document of a language client, which is `None` when it failed
  /// and why was logged.
  async fn format(
    &self,
    uri: &Uri,
    range: Option<Range>,
    use_global_config: bool,
    notify_no_config: bool,
    token: CancellationToken,
  ) -> Option<(Arc<LineIndex>, FormatOutcome)> {
    let (document, text, line_index) = self.ok_or_log_format_warning(self.get_document(uri))?;
    let ranges = match range {
      Some(range) => {
        if let Err(err) = line_index.get_text_range(range) {
          self.ok_or_log_format_warning::<()>(Err(anyhow::anyhow!("Invalid range for '{}'. {:#}", uri.as_str(), err)));
          return None;
        }
        let range = line_index.offset_utf16(range.start)..line_index.offset_utf16(range.end);
        Some(Vec::from([range]))
      }
      None => None,
    };
    let request = FormatRequest {
      document,
      text: text.to_string(),
      ranges,
      override_config: Default::default(),
      use_global_config,
      notify_no_config,
    };
    match self.format_request(request, token).await {
      Ok(outcome) => Some((line_index, outcome)),
      Err(err) => {
        self.log_format_failure(uri, &format!("{:#}", err));
        None
      }
    }
  }

  async fn format_request(&self, request: FormatRequest, token: CancellationToken) -> Result<FormatOutcome> {
    let start_time = std::time::Instant::now();
    let uri = request.document.uri.clone();
    log_debug!(self.environment, "Received format request for {}", uri.as_str());
    let token = Arc::new(token);
    let mut drop_token = DropToken::new(token.clone());
    let result = self
      .run(move |workspace| async move { workspace.format(request, &token).await }.boxed_local())
      .await
      .unwrap_or_else(|| Err(anyhow::anyhow!("The language server stopped.")));
    drop_token.completed();
    log_debug!(
      self.environment,
      "Finished format request for {} in {}ms",
      uri.as_str(),
      start_time.elapsed().as_millis()
    );
    result
  }

  /// Logs to stderr and the client that the document failed to format, which
  /// isn't a response error or a shown message because failing to format is
  /// what happens for a file with a syntax error, which would notify the user
  /// on every format on save.
  fn log_format_failure(&self, uri: &Uri, message: &str) {
    let message = format!("Failed formatting '{}': {}", uri.as_str(), message);
    log_error!(self.environment, "{}", message);
    self.client.log_error(message);
  }

  /// Gets the document of a language client along with its text and line
  /// index, which fails with the message to log when it can't be formatted.
  fn get_document(&self, uri: &Uri) -> Result<(DocumentInfo, Arc<String>, Arc<LineIndex>)> {
    let mut state = self.state.lock();
    let notebook_cell = state.documents.get_notebook_cell(uri);
    if !is_untitled_uri(uri) {
      match &notebook_cell {
        // the cli only formats notebooks on the file system
        Some((notebook_uri, _)) if uri_to_file_path(notebook_uri).is_none() => {
          bail!("Cannot format cell of a notebook that is not on the file system: {}", uri.as_str());
        }
        Some(_) => {}
        None => match uri_to_file_path(uri) {
          // the document is formatted with the config file for its directory
          Some(file_path) if file_path.parent().is_none() => bail!("Cannot format non-file path: {}", file_path.display()),
          Some(_) => {}
          None => bail!(
            "Cannot format document that is not a file, an untitled document or a cell of an open notebook: {}",
            uri.as_str()
          ),
        },
      }
    }
    let (text, line_index) = state.documents.get_content_with_line_index(uri)?;
    let (language_id, notebook_uri) = match notebook_cell {
      Some((notebook_uri, language_id)) => (Some(language_id), Some(notebook_uri)),
      None => (state.documents.get_language_id(uri), None),
    };
    let document = DocumentInfo {
      uri: uri.clone(),
      language: language_id.map(|id| DocumentLanguage {
        id,
        file_extensions: Vec::new(),
        file_names: Vec::new(),
      }),
      notebook_uri,
    };
    Ok((document, text, line_index))
  }

  /// Gets the value or otherwise logs to stderr and the client why the format
  /// request does nothing, which the client can't tell from the response.
  fn ok_or_log_format_warning<T>(&self, result: Result<T>) -> Option<T> {
    match result {
      Ok(value) => Some(value),
      Err(err) => {
        let message = format!("{:#}", err);
        log_warn!(self.environment, "{}", message);
        self.client.log_warn(message);
        None
      }
    }
  }

  /// Gets the text of a config file document for the completion and hover requests.
  fn get_config_content(&self, uri: &Uri) -> Option<(Arc<String>, Option<Arc<LineIndex>>)> {
    match self.state.lock().documents.get_content(uri) {
      Ok(content) => Some(content),
      Err(err) => {
        log_warn!(self.environment, "{:#}", err);
        None
      }
    }
  }

  async fn dprint_format(&self, params: FormatParams, token: CancellationToken) -> LspResult<FormatResult> {
    if let Some(key) = params.override_config.keys().find(|key| !OVERRIDE_CONFIG_KEYS.contains(&key.as_str())) {
      return Err(LspError {
        code: ErrorCode::InvalidParams,
        message: format!("Cannot override \"{}\". Only {} can be overridden.", key, OVERRIDE_CONFIG_KEYS.join(" and ")).into(),
        data: None,
      });
    }
    let line_index = LineIndex::new(&params.text);
    let request = FormatRequest {
      document: get_document_info(&params.document),
      ranges: params.ranges.map(|ranges| {
        ranges
          .iter()
          .map(|range| line_index.offset_utf16(range.start)..line_index.offset_utf16(range.end))
          .collect::<Vec<StdRange<u32>>>()
      }),
      text: params.text,
      override_config: params.override_config,
      use_global_config: params.document.use_global_config,
      notify_no_config: false,
    };
    let outcome = self.format_request(request, token).await.map_err(to_lsp_error)?;
    Ok(match outcome {
      FormatOutcome::Formatted(edits) => FormatResult::Formatted {
        edits: to_lsp_edits(edits, &line_index),
      },
      FormatOutcome::NotFormatted { reason, message, approval } => FormatResult::NotFormatted { reason, message, approval },
      FormatOutcome::Failed { reason, message } => FormatResult::Failed { reason, message },
    })
  }

  async fn dprint_file_info(&self, params: DocumentParams, _token: CancellationToken) -> LspResult<FileInfoResult> {
    let document = get_document_info(&params);
    let use_global_config = params.use_global_config;
    self
      .run(move |workspace| async move { workspace.file_info(&document, use_global_config).await }.boxed_local())
      .await
      .unwrap_or_else(|| Err(anyhow::anyhow!("The language server stopped.")))
      .map_err(to_lsp_error)
  }

  async fn dprint_config_info(&self, params: ConfigParams, _token: CancellationToken) -> LspResult<ConfigInfoResult> {
    self
      .run(move |workspace| async move { workspace.config_info(&params.uri, params.use_global_config).await }.boxed_local())
      .await
      .unwrap_or_else(|| Err(anyhow::anyhow!("The language server stopped.")))
      .map_err(to_lsp_error)
  }

  async fn dprint_config_schema(&self, params: ConfigParams, _token: CancellationToken) -> LspResult<ConfigSchemaResult> {
    let schema = self
      .run(move |workspace| async move { workspace.config_schema(&params.uri, params.use_global_config).await }.boxed_local())
      .await
      .ok_or_else(new_server_stopped_error)?;
    Ok(ConfigSchemaResult { schema })
  }

  async fn dprint_approve(&self, params: ApproveParams, _token: CancellationToken) -> LspResult<ApproveResult> {
    let approval = self
      .run(move |workspace| async move { workspace.approve(&params.id, params.decision).await }.boxed_local())
      .await
      .unwrap_or_else(|| Err(anyhow::anyhow!("The language server stopped.")))
      .map_err(to_lsp_error)?;
    Ok(ApproveResult { approval })
  }

  async fn dprint_reset(&self, _token: CancellationToken) -> LspResult<()> {
    self.run(|workspace| async move { workspace.reset().await }.boxed_local()).await;
    Ok(())
  }

  /// This is used in the test code to ensure there are no pending requests.
  #[cfg(test)]
  pub async fn has_pending(&self) -> bool {
    let (sender, receiver) = oneshot::channel();
    if self.sender.send(ChannelMessage::HasPending(sender)).is_ok() {
      receiver.await.unwrap_or(false)
    } else {
      false
    }
  }
}

#[deno_tower_lsp::async_trait(?Send)]
impl<TEnvironment: Environment> LanguageServer for Backend<TEnvironment> {
  async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
    if let Some(parent_id) = params.process_id {
      start_parent_process_checker_task(parent_id);
    }
    let extension_capability = params
      .capabilities
      .experimental
      .as_ref()
      .and_then(|experimental| experimental.get(EXPERIMENTAL_CAPABILITY_NAME))
      .and_then(|capability| serde_json::from_value::<ExperimentalCapability>(capability.clone()).ok());
    if let Some(capability) = &extension_capability {
      // a newer client only uses what this version supports
      log_debug!(
        self.environment,
        "The editor extension supports version {} of the dprint methods and the server supports version {}.",
        capability.version,
        EXPERIMENTAL_CAPABILITY_VERSION
      );
    }
    let is_editor_extension = extension_capability.is_some();
    let client_kind = if is_editor_extension {
      ClientKind::EditorExtension
    } else {
      ClientKind::LanguageClient {
        supports_message_actions: supports_message_actions(&params.capabilities),
      }
    };
    {
      let mut state = self.state.lock();
      if let Some(options) = &params.initialization_options {
        state.settings.update(options);
      }
      state.client_kind = client_kind;
      state.supports_document_changes = supports_document_changes(&params.capabilities);
      state.workspace_folders = get_workspace_folder_paths(params.workspace_folders.as_deref().unwrap_or_default());
      // an editor extension tells the server when a config file changes without this
      if !is_editor_extension {
        state.pending_registrations = get_config_file_watcher_registrations(&params.capabilities);
        state.pending_registrations.extend(get_notebook_cell_format_registrations(&params.capabilities));
        state.pending_registrations.extend(get_untitled_registrations(&params.capabilities));
        state
          .pending_registrations
          .extend(get_config_file_capabilities(&params.capabilities).registrations);
      }
    }
    if is_editor_extension {
      // what's logged and the progress go to the editor instead of stderr
      self.environment.set_output_sink(Arc::new(LspOutputSink::new(
        self.client.clone(),
        supports_work_done_progress(&params.capabilities),
      )));
    }
    self
      .run(move |workspace| {
        workspace.set_client_kind(client_kind);
        async {}.boxed_local()
      })
      .await;
    self.update_workspace_settings().await;

    let experimental = serde_json::json!({
      EXPERIMENTAL_CAPABILITY_NAME: ExperimentalCapability {
        version: EXPERIMENTAL_CAPABILITY_VERSION,
      },
    });
    let workspace = Some(WorkspaceServerCapabilities {
      workspace_folders: Some(WorkspaceFoldersServerCapabilities {
        supported: Some(true),
        change_notifications: Some(OneOf::Left(true)),
      }),
      file_operations: None,
    });
    let capabilities = if is_editor_extension {
      // an editor extension sends the text of a document when formatting it,
      // so the server doesn't need to keep the documents
      ServerCapabilities {
        workspace,
        experimental: Some(experimental),
        ..ServerCapabilities::default()
      }
    } else {
      let config_file_capabilities = get_config_file_capabilities(&params.capabilities);
      ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
          // todo: incremental should work now, but let's try out full to start
          change: Some(TextDocumentSyncKind::FULL),
          open_close: Some(true),
          save: None,
          will_save: None,
          will_save_wait_until: None,
        })),
        notebook_document_sync: Some(OneOf::Left(get_notebook_document_sync_options())),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        completion_provider: config_file_capabilities.completion_provider,
        hover_provider: config_file_capabilities.hover_provider,
        execute_command_provider: Some(ExecuteCommandOptions {
          commands: vec![
            FORMAT_WITH_GLOBAL_CONFIG_COMMAND.to_string(),
            FORMAT_SELECTION_WITH_GLOBAL_CONFIG_COMMAND.to_string(),
          ],
          work_done_progress_options: Default::default(),
        }),
        workspace,
        experimental: Some(experimental),
        ..ServerCapabilities::default()
      }
    };
    Ok(InitializeResult {
      server_info: Some(ServerInfo {
        name: "dprint".to_string(),
        version: Some(self.environment.cli_version()),
      }),
      capabilities,
    })
  }

  async fn initialized(&self, _: InitializedParams) {
    self.client.log_info(format!(
      "dprint {} ({}-{})",
      self.environment.cli_version(),
      self.environment.os(),
      self.environment.cpu_arch()
    ));
    self.client.log_info("Server ready.".to_string());
    let registrations = std::mem::take(&mut self.state.lock().pending_registrations);
    if !registrations.is_empty() {
      self.client.register_capabilities(registrations);
    }
  }

  async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
    self.state.lock().settings.update(&params.settings);
    self.update_workspace_settings().await;
  }

  async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
    let changes = params
      .changes
      .iter()
      .filter(|change| is_config_uri(&change.uri))
      .filter_map(|change| {
        let file_path = uri_to_file_path(&change.uri)?;
        // a config file in node_modules is only used when it's extended
        if file_path.components().any(|component| component.as_os_str() == "node_modules") {
          return None;
        }
        let change = match change.typ {
          FileChangeType::CREATED => ConfigFileChange::Created,
          FileChangeType::DELETED => ConfigFileChange::Deleted,
          _ => ConfigFileChange::Changed,
        };
        Some((file_path, change))
      })
      .collect::<Vec<_>>();
    if changes.is_empty() {
      return;
    }
    // a notification doesn't wait on this so that it doesn't hold up the other messages
    self.spawn(move |workspace| {
      workspace.config_files_changed(changes);
      async {}.boxed_local()
    });
  }

  async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
    let removed = get_workspace_folder_paths(&params.event.removed);
    let added = get_workspace_folder_paths(&params.event.added);
    {
      let mut state = self.state.lock();
      state.workspace_folders.retain(|folder| !removed.contains(folder));
      state.workspace_folders.extend(added);
    }
    self.update_workspace_settings().await;
  }

  async fn did_open(&self, params: DidOpenTextDocumentParams) {
    self.state.lock().documents.open(params.text_document);
  }

  async fn did_change(&self, params: DidChangeTextDocumentParams) {
    self.state.lock().documents.changed(params);
  }

  async fn did_close(&self, params: DidCloseTextDocumentParams) {
    self.state.lock().documents.closed(&params.text_document.uri);
  }

  async fn notebook_did_open(&self, params: DidOpenNotebookDocumentParams) {
    let mut state = self.state.lock();
    for cell_document in params.cell_text_documents {
      state.documents.open_notebook_cell(&params.notebook_document.uri, cell_document);
    }
  }

  async fn notebook_did_change(&self, params: DidChangeNotebookDocumentParams) {
    let Some(cells) = params.change.cells else {
      return;
    };
    let mut state = self.state.lock();
    if let Some(structure) = cells.structure {
      for cell_document in structure.did_close.unwrap_or_default() {
        state.documents.closed(&cell_document.uri);
      }
      for cell_document in structure.did_open.unwrap_or_default() {
        state.documents.open_notebook_cell(&params.notebook_document.uri, cell_document);
      }
    }
    for change in cells.text_content.unwrap_or_default() {
      state.documents.changed(DidChangeTextDocumentParams {
        text_document: change.document,
        content_changes: change.changes,
      });
    }
  }

  async fn notebook_did_close(&self, params: DidCloseNotebookDocumentParams) {
    self.state.lock().documents.closed_notebook(&params.notebook_document.uri);
  }

  async fn formatting(&self, params: DocumentFormattingParams, token: CancellationToken) -> LspResult<Option<Vec<TextEdit>>> {
    Ok(self.format_document(&params.text_document.uri, None, &params.options, token).await)
  }

  async fn range_formatting(&self, params: DocumentRangeFormattingParams, token: CancellationToken) -> LspResult<Option<Vec<TextEdit>>> {
    Ok(
      self
        .format_document(&params.text_document.uri, Some(params.range), &params.options, token)
        .await,
    )
  }

  async fn execute_command(&self, params: ExecuteCommandParams, token: CancellationToken) -> LspResult<Option<serde_json::Value>> {
    let has_range = match params.command.as_str() {
      FORMAT_WITH_GLOBAL_CONFIG_COMMAND => false,
      FORMAT_SELECTION_WITH_GLOBAL_CONFIG_COMMAND => true,
      command => return Err(LspError::invalid_params(format!("Unknown command: {}", command))),
    };
    let mut arguments = params.arguments.into_iter();
    let Some(uri) = arguments.next().and_then(|value| serde_json::from_value::<Uri>(value).ok()) else {
      return Err(LspError::invalid_params("Expected the first argument to be the uri of the document to format."));
    };
    let range = if has_range {
      let Some(range) = arguments.next().and_then(|value| serde_json::from_value::<Range>(value).ok()) else {
        return Err(LspError::invalid_params("Expected the second argument to be the range to format."));
      };
      Some(range)
    } else {
      None
    };
    self.format_with_global_config(&uri, range, token).await;
    Ok(None)
  }

  async fn completion(&self, params: CompletionParams, _token: CancellationToken) -> LspResult<Option<CompletionResponse>> {
    let uri = params.text_document_position.text_document.uri;
    if !is_config_uri(&uri) {
      return Ok(None);
    }
    let Some(file_path) = uri_to_file_path(&uri) else {
      return Ok(None);
    };
    let Some((file_text, _)) = self.get_config_content(&uri) else {
      return Ok(None);
    };
    let (sender, receiver) = oneshot::channel();
    let request = ConfigEditorRequest {
      file_path,
      file_text,
      position: params.text_document_position.position,
      use_global_config: self.state.lock().settings.use_global_config,
    };
    if self.sender.send(ChannelMessage::Completion(request, sender)).is_err() {
      return Ok(None);
    }
    Ok(receiver.await.ok().flatten().map(CompletionResponse::List))
  }

  async fn hover(&self, params: HoverParams, _token: CancellationToken) -> LspResult<Option<Hover>> {
    let uri = params.text_document_position_params.text_document.uri;
    if !is_config_uri(&uri) {
      return Ok(None);
    }
    let Some(file_path) = uri_to_file_path(&uri) else {
      return Ok(None);
    };
    let Some((file_text, _)) = self.get_config_content(&uri) else {
      return Ok(None);
    };
    let (sender, receiver) = oneshot::channel();
    let request = ConfigEditorRequest {
      file_path,
      file_text,
      position: params.text_document_position_params.position,
      use_global_config: self.state.lock().settings.use_global_config,
    };
    if self.sender.send(ChannelMessage::Hover(request, sender)).is_err() {
      return Ok(None);
    }
    Ok(receiver.await.ok().flatten())
  }

  async fn shutdown(&self) -> LspResult<()> {
    let (sender, receiver) = oneshot::channel();
    if self.sender.send(ChannelMessage::Shutdown(sender)).is_ok() {
      let _ = receiver.await;
    };
    Ok(())
  }
}

/// Sends what's logged and the progress to an editor extension from any
/// thread, which are sent in order.
struct LspOutputSink {
  sender: mpsc::UnboundedSender<OutputEvent>,
  /// The last percentage sent for each progress, which is `None` when it's
  /// not known.
  percentages: Mutex<HashMap<usize, Option<u32>>>,
  supports_progress: bool,
}

enum OutputEvent {
  Log(MessageType, String),
  Progress(usize, WorkDoneProgress),
}

impl LspOutputSink {
  fn new(client: ClientWrapper, supports_progress: bool) -> Self {
    let (sender, mut receiver) = mpsc::unbounded_channel::<OutputEvent>();
    dprint_core::async_runtime::spawn(async move {
      while let Some(event) = receiver.recv().await {
        match event {
          OutputEvent::Log(message_type, message) => client.send_log(message_type, message).await,
          OutputEvent::Progress(id, progress) => {
            let token = NumberOrString::String(format!("dprint/{}", id));
            // the client creates the progress before it's begun
            if matches!(progress, WorkDoneProgress::Begin(_)) && !client.create_work_done_progress(token.clone()).await {
              continue;
            }
            client.send_progress(token, progress).await;
          }
        }
      }
    });
    Self {
      sender,
      percentages: Default::default(),
      supports_progress,
    }
  }

  fn send_progress(&self, id: usize, progress: WorkDoneProgress) {
    if self.supports_progress {
      let _ = self.sender.send(OutputEvent::Progress(id, progress));
    }
  }
}

impl OutputSink for LspOutputSink {
  fn log(&self, level: LogLevel, text: &str) {
    let message_type = match level {
      LogLevel::Error => MessageType::ERROR,
      LogLevel::Warn => MessageType::WARNING,
      LogLevel::Info => MessageType::INFO,
      LogLevel::Debug => MessageType::LOG,
      LogLevel::Silent => return,
    };
    let _ = self.sender.send(OutputEvent::Log(message_type, text.to_string()));
  }

  fn start_progress(&self, id: usize, title: &str, total_size: usize) {
    let percentage = (total_size > 0).then_some(0);
    self.percentages.lock().insert(id, percentage);
    self.send_progress(
      id,
      WorkDoneProgress::Begin(WorkDoneProgressBegin {
        title: title.to_string(),
        cancellable: Some(false),
        message: None,
        percentage,
      }),
    );
  }

  fn update_progress(&self, id: usize, position: usize, total_size: usize) {
    if total_size == 0 {
      return;
    }
    let percentage = std::cmp::min(100, position as u128 * 100 / total_size as u128) as u32;
    {
      let mut percentages = self.percentages.lock();
      // only send when it changes because this is updated often (ex. for each part of a download)
      match percentages.get_mut(&id) {
        Some(previous) if *previous != Some(percentage) => *previous = Some(percentage),
        _ => return,
      }
    }
    self.send_progress(
      id,
      WorkDoneProgress::Report(WorkDoneProgressReport {
        cancellable: Some(false),
        message: None,
        percentage: Some(percentage),
      }),
    );
  }

  fn finish_progress(&self, id: usize) {
    if self.percentages.lock().remove(&id).is_some() {
      self.send_progress(id, WorkDoneProgress::End(WorkDoneProgressEnd { message: None }));
    }
  }
}

/// Attempts to convert a uri to a file path. By default, uses the Url
/// crate's `to_file_path()` method, but falls back to try and resolve unix-style
/// paths on Windows.
///
// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
// Lifted from code I wrote here:
// https://github.com/denoland/deno/blob/8702894feb480181040152a06e7c3eaf38619629/cli/util/path.rs#L85
pub fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
  let specifier = Url::parse(uri.as_str()).ok()?;
  if specifier.scheme() != "file" {
    return None;
  }

  match specifier.to_file_path() {
    Ok(path) => Some(path),
    Err(()) => {
      if cfg!(windows) {
        // This might be a unix-style path which is used in the tests even on Windows.
        // Attempt to see if we can convert it to a `PathBuf`. This code should be removed
        // once/if https://github.com/servo/rust-url/issues/730 is implemented.
        if specifier.scheme() == "file" && specifier.host().is_none() && specifier.port().is_none() && specifier.path_segments().is_some() {
          let path_str = specifier.path();
          String::from_utf8(percent_encoding::percent_decode(path_str.as_bytes()).collect())
            .ok()
            .map(PathBuf::from)
        } else {
          None
        }
      } else {
        None
      }
    }
  }
}

/// Gets the uri of a file path, which is the reverse of `uri_to_file_path`.
pub fn file_path_to_uri(file_path: &Path) -> Option<Uri> {
  let url = match Url::from_file_path(file_path) {
    Ok(url) => url,
    // a unix-style path, which is used in the tests even on Windows (see `uri_to_file_path`)
    Err(()) if cfg!(windows) && file_path.to_string_lossy().starts_with('/') => {
      Url::parse(&format!("file://{}", file_path.to_string_lossy().replace('\\', "/"))).ok()?
    }
    Err(()) => return None,
  };
  url.as_str().parse().ok()
}

/// Gets the document of a `dprint/*` method.
fn get_document_info(params: &DocumentParams) -> DocumentInfo {
  DocumentInfo {
    uri: params.text_document.uri.clone(),
    language: params.language.clone(),
    notebook_uri: params.notebook_document.as_ref().map(|notebook| notebook.uri.clone()),
  }
}

/// Gets the error response of a `dprint/*` method.
fn to_lsp_error(err: anyhow::Error) -> LspError {
  let code = if err.downcast_ref::<UnsupportedDocumentError>().is_some() {
    ErrorCode::InvalidParams
  } else if err.downcast_ref::<ApprovalChangedError>().is_some() {
    // what needs to be approved changed, which the client gets again
    ErrorCode::ContentModified
  } else {
    ErrorCode::InternalError
  };
  LspError {
    code,
    message: format!("{:#}", err).into(),
    data: None,
  }
}

fn new_server_stopped_error() -> LspError {
  to_lsp_error(anyhow::anyhow!("The language server stopped."))
}

fn has_use_global_config_option(options: &FormattingOptions) -> bool {
  matches!(options.properties.get(USE_GLOBAL_CONFIG_OPTION), Some(FormattingProperty::Bool(true)))
}

/// Creates the edit for the provided version of the document.
fn new_workspace_edit(uri: &Uri, version: Option<i32>, edits: Vec<TextEdit>, supports_document_changes: bool) -> WorkspaceEdit {
  if supports_document_changes {
    // provide the version so the client rejects the edits when the document
    // changed in the client and the server hasn't been told about it yet
    WorkspaceEdit {
      document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier { uri: uri.clone(), version },
        edits: edits.into_iter().map(OneOf::Left).collect(),
      }])),
      ..Default::default()
    }
  } else {
    WorkspaceEdit {
      changes: Some(HashMap::from([(uri.clone(), edits)])),
      ..Default::default()
    }
  }
}

/// Gets the registration that has the client tell the server when a config
/// file changes, which is when the client supports registering it.
fn get_config_file_watcher_registrations(capabilities: &ClientCapabilities) -> Vec<Registration> {
  let can_register = capabilities
    .workspace
    .as_ref()
    .and_then(|workspace| workspace.did_change_watched_files.as_ref())
    .and_then(|did_change_watched_files| did_change_watched_files.dynamic_registration)
    == Some(true);
  if !can_register {
    return Vec::new();
  }
  let options = DidChangeWatchedFilesRegistrationOptions {
    watchers: vec![FileSystemWatcher {
      glob_pattern: GlobPattern::String(CONFIG_FILE_GLOB.to_string()),
      kind: None,
    }],
  };
  vec![Registration {
    id: "dprint-config-file-watcher".to_string(),
    method: "workspace/didChangeWatchedFiles".to_string(),
    register_options: Some(serde_json::to_value(options).expect("options should serialize")),
  }]
}

fn supports_message_actions(capabilities: &ClientCapabilities) -> bool {
  // the `messageActionItem` property within this is only about the
  // additional properties of an action, which aren't used
  capabilities.window.as_ref().is_some_and(|window| window.show_message.is_some())
}

fn supports_work_done_progress(capabilities: &ClientCapabilities) -> bool {
  capabilities.window.as_ref().and_then(|window| window.work_done_progress).unwrap_or(false)
}

fn supports_document_changes(capabilities: &ClientCapabilities) -> bool {
  capabilities
    .workspace
    .as_ref()
    .and_then(|workspace| workspace.workspace_edit.as_ref())
    .and_then(|workspace_edit| workspace_edit.document_changes)
    .unwrap_or(false)
}

fn get_workspace_folder_paths(folders: &[WorkspaceFolder]) -> Vec<PathBuf> {
  folders.iter().filter_map(|folder| uri_to_file_path(&folder.uri)).collect()
}

#[cfg(test)]
mod test {
  mod editor_extension;

  use std::str::FromStr;
  use std::time::Duration;

  use deno_tower_lsp::lsp_types::ClientCapabilities;
  use deno_tower_lsp::lsp_types::Diagnostic;
  use deno_tower_lsp::lsp_types::DynamicRegistrationClientCapabilities;
  use deno_tower_lsp::lsp_types::MessageActionItem;
  use deno_tower_lsp::lsp_types::MessageType;
  use deno_tower_lsp::lsp_types::NotebookCellArrayChange;
  use deno_tower_lsp::lsp_types::NotebookDocument;
  use deno_tower_lsp::lsp_types::NotebookDocumentCellChange;
  use deno_tower_lsp::lsp_types::NotebookDocumentCellChangeStructure;
  use deno_tower_lsp::lsp_types::NotebookDocumentChangeEvent;
  use deno_tower_lsp::lsp_types::NotebookDocumentChangeTextContent;
  use deno_tower_lsp::lsp_types::NotebookDocumentIdentifier;
  use deno_tower_lsp::lsp_types::Position;
  use deno_tower_lsp::lsp_types::Range;
  use deno_tower_lsp::lsp_types::Registration;
  use deno_tower_lsp::lsp_types::ShowMessageRequestClientCapabilities;
  use deno_tower_lsp::lsp_types::TextDocumentClientCapabilities;
  use deno_tower_lsp::lsp_types::TextDocumentContentChangeEvent;
  use deno_tower_lsp::lsp_types::TextDocumentIdentifier;
  use deno_tower_lsp::lsp_types::TextDocumentItem;
  use deno_tower_lsp::lsp_types::TextDocumentPositionParams;
  use deno_tower_lsp::lsp_types::VersionedNotebookDocumentIdentifier;
  use deno_tower_lsp::lsp_types::VersionedTextDocumentIdentifier;
  use deno_tower_lsp::lsp_types::WindowClientCapabilities;
  use deno_tower_lsp::lsp_types::WorkspaceClientCapabilities;
  use deno_tower_lsp::lsp_types::WorkspaceEditClientCapabilities;
  use deno_tower_lsp::lsp_types::WorkspaceFoldersChangeEvent;
  use dprint_core::async_runtime::FutureExt;
  use dprint_core::async_runtime::LocalBoxFuture;
  use dprint_core::async_runtime::future;

  use crate::environment::TestConfigFileBuilder;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::plugins::PluginCache;

  use super::client::ClientTrait;
  use super::*;
  use crate::configuration::get_editor_approvals_file_path;
  use crate::configuration::get_settings_file_path;

  macro_rules! did_open {
    ($backend:expr, $uri:expr, $text:expr) => {
      $backend
        .did_open(DidOpenTextDocumentParams {
          text_document: TextDocumentItem {
            uri: $uri.clone(),
            language_id: "txt".to_string(),
            version: 0,
            text: $text.to_string(),
          },
        })
        .await;
    };
  }

  macro_rules! did_close {
    ($backend:expr, $uri:expr) => {
      $backend
        .did_close(DidCloseTextDocumentParams {
          text_document: TextDocumentIdentifier { uri: $uri.clone() },
        })
        .await;
    };
  }

  macro_rules! assert_format {
    ($backend:expr, $uri:expr, $expected:expr) => {
      let result = $backend
        .formatting(
          DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: $uri.clone() },
            options: Default::default(),
            work_done_progress_params: Default::default(),
          },
          CancellationToken::new(),
        )
        .await;
      assert_eq!(result.unwrap(), $expected);
    };
  }

  #[test]
  fn should_ask_to_download_from_domains_that_are_not_trusted_with_lsp() {
    const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
      .with_default_config(|c| {
        c.add_plugin(UNTRUSTED_PLUGIN_URL);
      })
      .build(); // not initialized, so the plugin isn't installed
    environment.write_file("/file.txt", "text").unwrap();

    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let (backend, recv_task, test_client) = setup_backend(environment.clone());
        test_client.set_message_action_selection("Download");
        let run_test_task = dprint_core::async_runtime::spawn({
          let test_client = test_client.clone();
          let environment = environment.clone();
          async move {
            initialize_backend(
              &backend,
              InitializeParams {
                capabilities: message_actions_capabilities(),
                ..Default::default()
              },
            )
            .await;
            let file_uri = Uri::from_str("file:///file.txt").unwrap();
            did_open!(backend, file_uri, "text");
            // nothing is downloaded until the user allows it
            assert_format!(backend, file_uri, None);
            assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 0);
            let shown = test_client.take_shown_messages();
            assert_eq!(shown.len(), 1, "{shown:?}");
            assert!(
              shown[0]
                .1
                .starts_with("dprint needs to download https://example.com/test-plugin.wasm from domains that aren't trusted.")
            );
            assert_eq!(shown[0].2, vec!["Download", "Cancel"]);
            // the user chose to download it
            let mut result = None;
            for _ in 0..100 {
              tokio::time::sleep(std::time::Duration::from_millis(10)).await;
              result = backend
                .formatting(
                  DocumentFormattingParams {
                    text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                    options: Default::default(),
                    work_done_progress_params: Default::default(),
                  },
                  CancellationToken::new(),
                )
                .await
                .unwrap();
              if result.is_some() {
                break;
              }
            }
            assert!(result.is_some());
            assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 1);
            // it's only asked once
            assert!(test_client.take_shown_messages().is_empty());
            backend.shutdown().await.unwrap();
          }
        });
        try_join!(recv_task, run_test_task).unwrap();
        test_client.take_messages();
      }
    });
    environment.take_stderr_messages();
  }

  #[test]
  fn should_format_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_includes("**/*.{txt,ts}")
          .add_excludes("ignored_file.txt")
          .add_excludes("ignored-dir");
      })
      .initialize()
      .build();
    crate::test_helpers::allow_process_plugins_in_editors(&environment);
    environment.write_file(".gitignore", "gitignored_file.txt\ngitignored_dir").unwrap();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 7), Position::new(0, 7)),
              new_text: "_formatted".to_string()
            }])
          );

          // update the document
          backend
            .did_change(DidChangeTextDocumentParams {
              text_document: VersionedTextDocumentIdentifier {
                uri: file_uri.clone(),
                version: 1,
              },
              content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 7), Position::new(0, 7))),
                range_length: None,
                text: "_formatted".to_string(),
              }],
            })
            .await;

          // format again, but it should be formatted now
          assert_format!(backend, file_uri, None);

          // change the text to an error
          backend
            .did_change(DidChangeTextDocumentParams {
              text_document: VersionedTextDocumentIdentifier {
                uri: file_uri.clone(),
                version: 1,
              },
              content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 0), Position::new(0, 17))),
                range_length: None,
                text: "plugin: should_error".to_string(),
              }],
            })
            .await;
          assert_format!(backend, file_uri, None);
          assert_eq!(
            environment.take_stderr_messages(),
            vec!["Failed formatting 'file:///file.txt': Did error.".to_string()],
          );

          let mut handles = Vec::new();
          for i in 0..50 {
            let file_uri = Uri::from_str(&format!("file:///file_{}.txt", i)).unwrap();
            let backend = backend.clone();
            handles.push(dprint_core::async_runtime::spawn(async move {
              let file_text = format!("testing_{}", i);
              backend
                .did_open(DidOpenTextDocumentParams {
                  text_document: TextDocumentItem {
                    uri: file_uri.clone(),
                    language_id: "txt".to_string(),
                    version: 0,
                    text: file_text.clone(),
                  },
                })
                .await;
              let result = backend
                .formatting(
                  DocumentFormattingParams {
                    text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                    options: Default::default(),
                    work_done_progress_params: Default::default(),
                  },
                  CancellationToken::new(),
                )
                .await;
              assert_eq!(
                result.unwrap().unwrap(),
                vec![TextEdit {
                  range: Range::new(Position::new(0, file_text.len() as u32), Position::new(0, file_text.len() as u32),),
                  new_text: "_formatted".to_string()
                }]
              );

              // test closing too
              backend
                .did_close(DidCloseTextDocumentParams {
                  text_document: TextDocumentIdentifier { uri: file_uri },
                })
                .await;
            }))
          }

          // ensure nothing panicked
          let results = future::join_all(handles).await;
          for result in results {
            result.unwrap();
          }

          // ignores excluded files
          let file_uri = Uri::from_str("file:///ignored_file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          // ignores gitignored files
          let file_uri = Uri::from_str("file:///gitignored_file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          // ignores file in gitignored dir
          let file_uri = Uri::from_str("file:///gitignored_dir/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          // ignores excluded directory files
          let file_uri = Uri::from_str("file:///ignored-dir/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          // ignores non-included files
          let file_uri = Uri::from_str("file:///file.txt_ps").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          // now update the config file to include it by removing the includes,
          // which it should in the range formatting
          {
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file.add_remote_wasm_plugin().add_remote_process_plugin();
            environment.write_file("/dprint.json", &config_file.to_string()).unwrap();
          }

          // range formatting
          let result = backend
            .range_formatting(
              DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                range: Range {
                  start: Position { line: 0, character: 1 },
                  end: Position { line: 0, character: 2 },
                },
                options: Default::default(),
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await;
          // the process plugin formats the range itself
          assert_eq!(
            result.unwrap().unwrap(),
            vec![
              TextEdit {
                range: Range::new(Position::new(0, 1), Position::new(0, 1)),
                new_text: "_formatted_proc".to_string()
              },
              TextEdit {
                range: Range::new(Position::new(0, 3), Position::new(0, 3)),
                new_text: "s_s".to_string()
              },
              TextEdit {
                range: Range::new(Position::new(0, 7), Position::new(0, 7)),
                new_text: "_formatted_process".to_string()
              },
            ]
          );

          // cancellation via a drop
          let file_uri = Uri::from_str("file:///file_cancellation.txt_ps").unwrap();
          did_open!(backend, file_uri, "wait_cancellation");

          let token = Arc::new(CancellationToken::new());
          dprint_core::async_runtime::spawn({
            let backend = backend.clone();
            let token = token.clone();
            async move {
              let future = backend.formatting(
                DocumentFormattingParams {
                  text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                  options: Default::default(),
                  work_done_progress_params: Default::default(),
                },
                CancellationToken::new(),
              );
              // this token's cancellation will drop the future which
              // will cancel the formatting request internally
              tokio::select! {
                _ = future => {},
                _ = token.cancelled() => {}
              }
            }
          });

          // give some time for the message to be sent
          tokio::time::sleep(Duration::from_millis(50)).await;
          assert!(backend.has_pending().await);
          token.cancel();
          // give some time for the message to be cancelled
          tokio::time::sleep(Duration::from_millis(50)).await;
          assert!(!backend.has_pending().await);

          // create a config file with associations
          {
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file
              .add_remote_wasm_plugin()
              .add_remote_process_plugin()
              .add_config_section(
                "test-plugin",
                r#"{
                "associations": [
                  "**/*.{txt,txt_ps}",
                  "some_file_name",
                  "test-process-plugin-exact-file"
                ],
                "ending": "wasm"
              }"#,
              )
              .add_config_section(
                "testProcessPlugin",
                r#"{
                "associations": [
                  "**/*.{txt,txt_ps,other}",
                  "test-process-plugin-exact-file"
                ]
                "ending": "ps"
              }"#,
              );
            environment.write_file("/dprint.json", &config_file.to_string()).unwrap();
          }

          // format using it
          let file_uri = Uri::from_str("file:///associations1.txt").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_wasm_ps".to_string()
            }])
          );
          backend
            .did_change(DidChangeTextDocumentParams {
              text_document: VersionedTextDocumentIdentifier {
                uri: file_uri.clone(),
                version: 1,
              },
              content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 0), Position::new(0, 4))),
                range_length: None,
                text: "plugin: text6".to_string(),
              }],
            })
            .await;
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 13), Position::new(0, 13)),
              new_text: "_wasm_ps_wasm_ps_ps".to_string()
            }])
          );

          // try .txt_ps file, which should act the same as above because
          // of the associations
          let file_uri = Uri::from_str("file:///associations1.txt_ps").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_wasm_ps".to_string()
            }])
          );

          // try the .other extension
          let file_uri = Uri::from_str("file:///dir/file.other").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_ps".to_string()
            }])
          );

          // try the exact file with no extension
          let file_uri = Uri::from_str("file:///dir/some_file_name").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_wasm".to_string()
            }])
          );

          // now try this special file name
          let file_uri = Uri::from_str("file:///dir/test-process-plugin-exact-file").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_wasm_ps".to_string()
            }])
          );

          // rewrite config with an override for package.txt
          {
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file.add_remote_wasm_plugin().add_config_section(
              "test-plugin",
              r#"{
                "ending": "base",
                "overrides": {
                  "files": "**/package.txt",
                  "ending": "package"
                }
              }"#,
            );
            environment.write_file("/dprint.json", &config_file.to_string()).unwrap();
          }

          let file_uri = Uri::from_str("file:///package.txt").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_package".to_string()
            }])
          );

          // now ensure formatting works with a sub folder config file that has different config
          {
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file.add_remote_wasm_plugin().add_remote_process_plugin();
            environment.mk_dir_all("/other_config").unwrap();
            environment.write_file("/other_config/dprint.json", &config_file.to_string()).unwrap();
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file
              .add_remote_wasm_plugin()
              .add_remote_process_plugin()
              .add_config_section("test-plugin", r#"{"ending": "asdf"}"#)
              .add_config_section("testProcessPlugin", r#"{"ending": "asdf_ps"}"#);
            environment.mk_dir_all("/other_config/sub").unwrap();
            environment.write_file("/other_config/sub/dprint.json", &config_file.to_string()).unwrap();
          }

          for _ in 0..2 {
            let file_uri = Uri::from_str("file:///other_config/file.txt").unwrap();
            did_open!(backend, file_uri, "text");
            assert_format!(
              backend,
              file_uri,
              Some(vec![TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                new_text: "_formatted".to_string()
              }])
            );
            did_close!(backend, file_uri);
            // switching to a different config should still work fine
            let file_uri = Uri::from_str("file:///other_config/sub/file.txt").unwrap();
            did_open!(backend, file_uri, "text");
            assert_format!(
              backend,
              file_uri,
              Some(vec![TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                new_text: "_asdf".to_string()
              }])
            );
            did_close!(backend, file_uri);

            // now try with a process plugin
            let file_uri = Uri::from_str("file:///other_config/file.txt_ps").unwrap();
            did_open!(backend, file_uri, "text");
            assert_format!(
              backend,
              file_uri,
              Some(vec![TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                new_text: "_formatted_process".to_string()
              }])
            );
            did_close!(backend, file_uri);
            // switching to a different config should work fine as well
            let file_uri = Uri::from_str("file:///other_config/sub/file.txt_ps").unwrap();
            did_open!(backend, file_uri, "text");
            assert_format!(
              backend,
              file_uri,
              Some(vec![TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                new_text: "_asdf_ps".to_string()
              }])
            );
            did_close!(backend, file_uri);
          }

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string()),
          // the client is told about the formatting failure
          (MessageType::ERROR, "Failed formatting 'file:///file.txt': Did error.".to_string()),
        ]
      );
    });
  }

  #[test]
  fn should_inherit_ancestor_config_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "root" }"#);
      })
      .with_local_config("/inherits/dprint.json", |c| {
        c.set_inherit(true);
      })
      .with_local_config("/inherits/overrides/dprint.json", |c| {
        c.set_inherit(true).add_config_section("test-plugin", r#"{ "ending": "nested" }"#);
      })
      .with_local_config("/no_inherit/dprint.json", |_| {})
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          macro_rules! assert_ending {
            ($uri:expr, $ending:expr) => {
              let file_uri = Uri::from_str($uri).unwrap();
              did_open!(backend, file_uri, "text");
              assert_format!(
                backend,
                file_uri,
                Some(vec![TextEdit {
                  range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                  new_text: $ending.to_string(),
                }])
              );
            };
          }

          assert_ending!("file:///file.txt", "_root");
          // inherits the plugins and their configuration from the ancestor config
          assert_ending!("file:///inherits/file.txt", "_root");
          // inherits through a config that also inherits and overrides the configuration
          assert_ending!("file:///inherits/overrides/file.txt", "_nested");

          // a config that doesn't inherit has no plugins
          let file_uri = Uri::from_str("file:///no_inherit/file.txt").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(backend, file_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_format_with_other_configs_after_a_config_changes_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_process_plugin();
      })
      .with_local_config("/b/dprint.json", |c| {
        c.add_remote_process_plugin();
      })
      .initialize()
      .build();
    crate::test_helpers::allow_process_plugins_in_editors(&environment);

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          macro_rules! assert_ending {
            ($uri:expr, $ending:expr) => {
              let result = tokio::time::timeout(
                Duration::from_secs(10),
                backend.formatting(
                  DocumentFormattingParams {
                    text_document: TextDocumentIdentifier { uri: $uri.clone() },
                    options: Default::default(),
                    work_done_progress_params: Default::default(),
                  },
                  CancellationToken::new(),
                ),
              )
              .await
              .expect("timed out formatting");
              assert_eq!(
                result.unwrap(),
                Some(vec![TextEdit {
                  range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                  new_text: $ending.to_string(),
                }])
              );
            };
          }

          let a_uri = Uri::from_str("file:///a/file.txt_ps").unwrap();
          did_open!(backend, a_uri, "text");
          let b_uri = Uri::from_str("file:///b/file.txt_ps").unwrap();
          did_open!(backend, b_uri, "text");
          assert_ending!(a_uri, "_formatted_process");
          assert_ending!(b_uri, "_formatted_process");
          assert_eq!(environment.take_stderr_messages(), vec!["Extracting zip for test-process-plugin".to_string()]);

          // change one of the configs
          {
            let mut config_file = TestConfigFileBuilder::new(environment.clone());
            config_file
              .add_remote_process_plugin()
              .add_config_section("testProcessPlugin", r#"{ "ending": "changed" }"#);
            environment.write_file("/a/dprint.json", &config_file.to_string()).unwrap();
          }
          assert_ending!(a_uri, "_changed");

          // the other config's plugins were shut down along with the changed one's,
          // so ensure formatting with it still works and keeps working
          assert_ending!(b_uri, "_formatted_process");
          assert_ending!(b_uri, "_formatted_process");
          assert_ending!(a_uri, "_changed");
          assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_pick_up_gitignore_changes_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          macro_rules! assert_formats {
            ($uri:expr, $formats:expr) => {
              let expected = if $formats {
                Some(vec![TextEdit {
                  range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                  new_text: "_formatted".to_string(),
                }])
              } else {
                None
              };
              assert_format!(backend, $uri, expected);
            };
          }

          let dist_uri = Uri::from_str("file:///dist/file.txt").unwrap();
          did_open!(backend, dist_uri, "text");
          let src_uri = Uri::from_str("file:///src/file.txt").unwrap();
          did_open!(backend, src_uri, "text");
          let nested_uri = Uri::from_str("file:///src/nested/file.txt").unwrap();
          did_open!(backend, nested_uri, "text");

          // no gitignore yet
          assert_formats!(dist_uri, true);
          assert_formats!(src_uri, true);
          assert_formats!(nested_uri, true);

          // adding a gitignore without touching the dprint config
          environment.write_file("/.gitignore", "dist/").unwrap();
          assert_formats!(dist_uri, false);
          assert_formats!(src_uri, true);
          assert_formats!(nested_uri, true);

          // changing its entries
          environment.write_file("/.gitignore", "src/file.txt").unwrap();
          assert_formats!(dist_uri, true);
          assert_formats!(src_uri, false);
          assert_formats!(nested_uri, true);

          // adding a nested gitignore
          environment.mk_dir_all("/src/nested").unwrap();
          environment.write_file("/src/nested/.gitignore", "file.txt").unwrap();
          assert_formats!(dist_uri, true);
          assert_formats!(src_uri, false);
          assert_formats!(nested_uri, false);

          // removing the gitignores
          environment.remove_file("/.gitignore").unwrap();
          assert_formats!(src_uri, true);
          assert_formats!(nested_uri, false);
          environment.remove_file("/src/nested/.gitignore").unwrap();
          assert_formats!(nested_uri, true);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_pick_up_repo_root_gitignore_changes_above_config_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/repo/sub/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    // the repository root is above the directory of the config
    environment.mk_dir_all("/repo/.git/info").unwrap();
    environment.write_file("/repo/.gitignore", "other/").unwrap();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          macro_rules! assert_formats {
            ($uri:expr, $formats:expr) => {
              let expected = if $formats {
                Some(vec![TextEdit {
                  range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                  new_text: "_formatted".to_string(),
                }])
              } else {
                None
              };
              assert_format!(backend, $uri, expected);
            };
          }

          let dist_uri = Uri::from_str("file:///repo/sub/dist/file.txt").unwrap();
          did_open!(backend, dist_uri, "text");
          let src_uri = Uri::from_str("file:///repo/sub/src/file.txt").unwrap();
          did_open!(backend, src_uri, "text");

          assert_formats!(dist_uri, true);
          assert_formats!(src_uri, true);
          assert_eq!(
            environment.take_stderr_messages(),
            vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string()]
          );

          // changing the gitignore at the repository root
          environment.write_file("/repo/.gitignore", "sub/dist/").unwrap();
          assert_formats!(dist_uri, false);
          assert_formats!(src_uri, true);

          // adding and then changing the repository's exclude file
          environment.write_file("/repo/.git/info/exclude", "sub/src/").unwrap();
          assert_formats!(dist_uri, false);
          assert_formats!(src_uri, false);
          environment.write_file("/repo/.git/info/exclude", "sub/other/").unwrap();
          assert_formats!(dist_uri, false);
          assert_formats!(src_uri, true);

          // a gitignore above the repository root doesn't apply
          environment.write_file("/.gitignore", "file.txt").unwrap();
          assert_formats!(dist_uri, false);
          assert_formats!(src_uri, true);

          // removing the gitignore at the repository root
          environment.remove_file("/repo/.gitignore").unwrap();
          assert_formats!(dist_uri, true);
          assert_formats!(src_uri, true);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_format_shebang_file_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section(
          "shebangs",
          r##"{
            "#!/bin/sh": "txt"
          }"##,
        );
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          // extensionless file with a matching shebang
          let file_uri = Uri::from_str("file:///scripts/build").unwrap();
          did_open!(backend, file_uri, "#!/bin/sh\ntext");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(1, 4), Position::new(1, 4)),
              new_text: "_formatted".to_string()
            }])
          );

          // extensionless file without a shebang
          let file_uri = Uri::from_str("file:///scripts/notes").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(backend, file_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_format_notebook_cells_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        // the process plugin stands in for the jupyter plugin
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "formatted\n" }"#)
          .add_config_section("testProcessPlugin", r#"{ "associations": ["**/*.ipynb"] }"#)
          .add_excludes("ignored-dir");
      })
      .initialize()
      .build();
    crate::test_helpers::allow_process_plugins_in_editors(&environment);

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          let result = backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              capabilities: ClientCapabilities {
                notebook_document: Some(Default::default()),
                text_document: Some(TextDocumentClientCapabilities {
                  formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(true),
                  }),
                  range_formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(true),
                  }),
                  ..Default::default()
                }),
                ..Default::default()
              },
              ..Default::default()
            })
            .await
            .unwrap();
          assert!(result.capabilities.notebook_document_sync.is_some());
          backend.initialized(InitializedParams {}).await;

          fn cell_document(uri: &Uri, language_id: &str, text: &str) -> TextDocumentItem {
            TextDocumentItem {
              uri: uri.clone(),
              language_id: language_id.to_string(),
              version: 0,
              text: text.to_string(),
            }
          }

          macro_rules! open_notebook {
            ($uri:expr, $cell_documents:expr) => {
              backend
                .notebook_did_open(DidOpenNotebookDocumentParams {
                  notebook_document: NotebookDocument {
                    uri: $uri.clone(),
                    notebook_type: "jupyter-notebook".to_string(),
                    version: 0,
                    metadata: None,
                    // the server only uses the documents of the cells
                    cells: Vec::new(),
                  },
                  cell_text_documents: $cell_documents,
                })
                .await;
            };
          }

          macro_rules! change_notebook {
            ($uri:expr, $cells:expr) => {
              backend
                .notebook_did_change(DidChangeNotebookDocumentParams {
                  notebook_document: VersionedNotebookDocumentIdentifier {
                    version: 1,
                    uri: $uri.clone(),
                  },
                  change: NotebookDocumentChangeEvent {
                    metadata: None,
                    cells: Some($cells),
                  },
                })
                .await;
            };
          }

          let notebook_uri = Uri::from_str("file:///dir/notebook.ipynb").unwrap();
          let cell_uri = Uri::from_str("vscode-notebook-cell:/dir/notebook.ipynb#W0sZmlsZQ%3D%3D").unwrap();
          let range_cell_uri = Uri::from_str("vscode-notebook-cell:/dir/notebook.ipynb#W1sZmlsZQ%3D%3D").unwrap();
          let python_cell_uri = Uri::from_str("vscode-notebook-cell:/dir/notebook.ipynb#W2sZmlsZQ%3D%3D").unwrap();
          open_notebook!(
            notebook_uri,
            vec![
              cell_document(&cell_uri, "txt", "text"),
              cell_document(&range_cell_uri, "txt", "text  "),
              cell_document(&python_cell_uri, "python", "text"),
            ]
          );

          // formats as the cell's language and trims the final newline
          assert_format!(
            backend,
            cell_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_formatted".to_string()
            }])
          );

          // the range is expanded to its whole lines, which is the whole cell here
          let result = backend
            .range_formatting(
              DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri: range_cell_uri.clone() },
                range: Range::new(Position::new(0, 1), Position::new(0, 2)),
                options: Default::default(),
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await;
          assert_eq!(
            result.unwrap(),
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 6), Position::new(0, 6)),
              new_text: "_formatted".to_string()
            }])
          );

          // language without a plugin
          assert_format!(backend, python_cell_uri, None);

          // changing a cell's text
          change_notebook!(
            notebook_uri,
            NotebookDocumentCellChange {
              structure: None,
              data: None,
              text_content: Some(vec![NotebookDocumentChangeTextContent {
                document: VersionedTextDocumentIdentifier {
                  uri: cell_uri.clone(),
                  version: 1,
                },
                changes: vec![TextDocumentContentChangeEvent {
                  range: Some(Range::new(Position::new(0, 0), Position::new(0, 4))),
                  range_length: None,
                  text: "changed".to_string(),
                }],
              }]),
            }
          );
          assert_format!(
            backend,
            cell_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 7), Position::new(0, 7)),
              new_text: "_formatted".to_string()
            }])
          );

          // adding and removing cells
          let added_cell_uri = Uri::from_str("vscode-notebook-cell:/dir/notebook.ipynb#W3sZmlsZQ%3D%3D").unwrap();
          change_notebook!(
            notebook_uri,
            NotebookDocumentCellChange {
              structure: Some(NotebookDocumentCellChangeStructure {
                array: NotebookCellArrayChange {
                  start: 1,
                  delete_count: 1,
                  cells: None,
                },
                did_open: Some(vec![cell_document(&added_cell_uri, "txt", "added")]),
                did_close: Some(vec![TextDocumentIdentifier { uri: range_cell_uri.clone() }]),
              }),
              data: None,
              text_content: None,
            }
          );
          assert_format!(
            backend,
            added_cell_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 5), Position::new(0, 5)),
              new_text: "_formatted".to_string()
            }])
          );
          assert_format!(backend, range_cell_uri, None);

          // excluded notebook
          let other_notebook_uri = Uri::from_str("file:///ignored-dir/notebook.ipynb").unwrap();
          let other_cell_uri = Uri::from_str("vscode-notebook-cell:/ignored-dir/notebook.ipynb#W0sZmlsZQ%3D%3D").unwrap();
          open_notebook!(other_notebook_uri, vec![cell_document(&other_cell_uri, "txt", "text")]);
          assert_format!(backend, other_cell_uri, None);

          // notebook without a plugin
          let other_notebook_uri = Uri::from_str("file:///dir/notebook.other").unwrap();
          let other_cell_uri = Uri::from_str("vscode-notebook-cell:/dir/notebook.other#W0sZmlsZQ%3D%3D").unwrap();
          open_notebook!(other_notebook_uri, vec![cell_document(&other_cell_uri, "txt", "text")]);
          assert_format!(backend, other_cell_uri, None);

          // notebook that's not on the file system
          let other_notebook_uri = Uri::from_str("untitled:Untitled-1.ipynb").unwrap();
          let other_cell_uri = Uri::from_str("vscode-notebook-cell:Untitled-1.ipynb#W0sdW50aXRsZWQ%3D").unwrap();
          open_notebook!(other_notebook_uri, vec![cell_document(&other_cell_uri, "txt", "text")]);
          assert_format!(backend, other_cell_uri, None);

          // closing the notebook closes its cells
          backend
            .notebook_did_close(DidCloseNotebookDocumentParams {
              notebook_document: NotebookDocumentIdentifier { uri: notebook_uri.clone() },
              cell_text_documents: vec![
                TextDocumentIdentifier { uri: cell_uri.clone() },
                TextDocumentIdentifier { uri: python_cell_uri.clone() },
                TextDocumentIdentifier { uri: added_cell_uri.clone() },
              ],
            })
            .await;
          assert_format!(backend, cell_uri, None);
          assert_format!(backend, added_cell_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      // the cells that were closed and the cell of the notebook that's not on the file system
      let closed_cell_warning = |cell_uri: &str| {
        format!(
          "Cannot format document that is not a file, an untitled document or a cell of an open notebook: {}",
          cell_uri
        )
      };
      let warnings = vec![
        closed_cell_warning("vscode-notebook-cell:/dir/notebook.ipynb#W1sZmlsZQ%3D%3D"),
        "Cannot format cell of a notebook that is not on the file system: vscode-notebook-cell:Untitled-1.ipynb#W0sdW50aXRsZWQ%3D".to_string(),
        closed_cell_warning("vscode-notebook-cell:/dir/notebook.ipynb#W0sZmlsZQ%3D%3D"),
        closed_cell_warning("vscode-notebook-cell:/dir/notebook.ipynb#W3sZmlsZQ%3D%3D"),
      ];
      assert_eq!(environment.take_stderr_messages(), warnings);
      assert_eq!(
        test_client.take_messages(),
        [
          vec![
            (
              MessageType::INFO,
              format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
            ),
            (MessageType::INFO, "Server ready.".to_string())
          ],
          warnings.into_iter().map(|message| (MessageType::WARNING, message)).collect(),
        ]
        .concat()
      );
      // the server has the client send it format requests for notebook cells
      assert_eq!(
        test_client.take_registered_methods(),
        vec!["textDocument/formatting", "textDocument/rangeFormatting"]
      );
    });
  }

  #[test]
  fn should_format_with_lsp_using_global_config() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt").add_excludes("ignored_file.txt");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              initialization_options: Some(serde_json::json!({ "useGlobalConfig": true })),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          // Test that global config is being used
          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 7), Position::new(0, 7)),
              new_text: "_formatted".to_string()
            }])
          );

          // Test that excluded file is not formatted
          let ignored_uri = Uri::from_str("file:///ignored_file.txt").unwrap();
          did_open!(backend, ignored_uri, "testing");
          assert_format!(backend, ignored_uri, None);

          // Test that non-matching file is not formatted
          let other_uri = Uri::from_str("file:///file.js").unwrap();
          did_open!(backend, other_uri, "testing");
          assert_format!(backend, other_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_format_files_outside_cwd_with_lsp_using_global_config() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt").add_excludes("**/ignored_file.txt");
      })
      .initialize()
      .write_file("/project/file.txt", "")
      .set_cwd("/project")
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              initialization_options: Some(serde_json::json!({ "useGlobalConfig": true })),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let formatted = Some(vec![TextEdit {
            range: Range::new(Position::new(0, 7), Position::new(0, 7)),
            new_text: "_formatted".to_string(),
          }]);

          // file in the cwd
          let file_uri = Uri::from_str("file:///project/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, formatted.clone());

          // file outside the cwd
          let file_uri = Uri::from_str("file:///other/dir/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, formatted);

          // the excludes still apply outside the cwd
          let file_uri = Uri::from_str("file:///other/dir/ignored_file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_not_use_global_config_with_lsp_by_default() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);
          assert_eq!(
            environment.take_stderr_messages(),
            vec!["Path did not have a dprint config file: /file.txt".to_string()]
          );

          // uses the global config when the client asks to
          let options = FormattingOptions {
            properties: HashMap::from([("useGlobalConfig".to_string(), FormattingProperty::Bool(true))]),
            ..Default::default()
          };
          let formatted = Some(vec![TextEdit {
            range: Range::new(Position::new(0, 7), Position::new(0, 7)),
            new_text: "_formatted".to_string(),
          }]);
          let result = backend
            .formatting(
              DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                options: options.clone(),
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await;
          assert_eq!(result.unwrap(), formatted);
          let result = backend
            .range_formatting(
              DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                range: Range::new(Position::new(0, 0), Position::new(0, 7)),
                options,
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await;
          assert!(result.unwrap().is_some());

          // uses the global config once the client changes the setting
          backend
            .did_change_configuration(DidChangeConfigurationParams {
              settings: serde_json::json!({ "dprint": { "useGlobalConfig": true } }),
            })
            .await;
          assert_format!(backend, file_uri, formatted);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
      // says how to use the global config file
      assert_eq!(
        test_client.take_shown_messages(),
        vec![(
          MessageType::INFO,
          "No dprint configuration file found. Run \"dprint init\" in your project to create one or enable the \"useGlobalConfig\" setting of the dprint language server to use your global one.".to_string(),
          Vec::new()
        )]
      );
    });
  }

  #[test]
  fn should_use_global_config_with_lsp_when_env_var_set() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt");
      })
      .initialize()
      .build();
    environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some("true"));

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn(async move {
        initialize_backend(&backend, Default::default()).await;

        let file_uri = Uri::from_str("file:///file.txt").unwrap();
        did_open!(backend, file_uri, "testing");
        assert_format!(
          backend,
          file_uri,
          Some(vec![TextEdit {
            range: Range::new(Position::new(0, 7), Position::new(0, 7)),
            new_text: "_formatted".to_string()
          }])
        );

        // the client's setting takes priority
        backend
          .did_change_configuration(DidChangeConfigurationParams {
            settings: serde_json::json!({ "useGlobalConfig": false }),
          })
          .await;
        assert_format!(backend, file_uri, None);

        backend.shutdown().await.unwrap();
      });

      try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      assert_eq!(
        environment.take_stderr_messages(),
        vec!["Path did not have a dprint config file: /file.txt".to_string()]
      );
    });
  }

  #[test]
  fn should_notify_once_when_no_config_file_with_lsp() {
    let environment = TestEnvironmentBuilder::new().build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn({
        let test_client = test_client.clone();
        async move {
          initialize_backend(
            &backend,
            InitializeParams {
              capabilities: message_actions_capabilities(),
              ..Default::default()
            },
          )
          .await;

          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);
          // there's no workspace folder to not show it in
          assert_eq!(
            test_client.take_shown_messages(),
            vec![(
              MessageType::INFO,
              "No dprint configuration file found. Run \"dprint init\" in your project to create one.".to_string(),
              vec!["Don't show again".to_string()]
            )]
          );

          // only notifies once per session
          assert_format!(backend, file_uri, None);
          let result = backend
            .range_formatting(
              DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                range: Range::new(Position::new(0, 0), Position::new(0, 7)),
                options: Default::default(),
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await;
          assert_eq!(result.unwrap(), None);
          assert_eq!(test_client.take_shown_messages(), Vec::new());

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_not_notify_when_no_config_file_with_lsp_when_setting_disabled() {
    let environment = TestEnvironmentBuilder::new().build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn({
        let test_client = test_client.clone();
        async move {
          initialize_backend(
            &backend,
            InitializeParams {
              initialization_options: Some(serde_json::json!({ "showNoConfigNotification": false })),
              ..Default::default()
            },
          )
          .await;

          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);
          assert_eq!(test_client.take_shown_messages(), Vec::new());

          // notifies once the client enables the setting
          backend
            .did_change_configuration(DidChangeConfigurationParams {
              settings: serde_json::json!({ "dprint": { "showNoConfigNotification": true } }),
            })
            .await;
          assert_format!(backend, file_uri, None);
          assert_eq!(test_client.take_shown_messages().len(), 1);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_ask_to_allow_process_plugins_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_process_plugin();
      })
      .build(); // not initialized, so the process plugin is only set up when it's allowed
    disallow_process_plugins(&environment);
    environment.write_file("/file.txt_ps", "text").unwrap();

    environment.clone().run_in_runtime(async move {
      // each backend is a session and returns the messages it showed along
      // with whether the process plugin was set up, which runs it
      async fn run_session(
        environment: &TestEnvironment,
        capabilities: ClientCapabilities,
        selection: Option<&str>,
        expected_formats: [bool; 2],
      ) -> (Vec<(MessageType, String, Vec<String>)>, bool) {
        let (backend, recv_task, test_client) = setup_backend(environment.clone());
        if let Some(selection) = selection {
          test_client.set_message_action_selection(selection);
        }
        let run_test_task = dprint_core::async_runtime::spawn(async move {
          initialize_backend(
            &backend,
            InitializeParams {
              capabilities,
              ..Default::default()
            },
          )
          .await;
          let file_uri = Uri::from_str("file:///file.txt_ps").unwrap();
          did_open!(backend, file_uri, "text");
          for formats in expected_formats {
            let expected = formats.then(|| {
              vec![TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 4)),
                new_text: "_formatted_process".to_string(),
              }]
            });
            assert_format!(backend, file_uri, expected);
            // let the task that stores the user's selection run
            tokio::task::yield_now().await;
          }
          backend.shutdown().await.unwrap();
        });
        try_join!(recv_task, run_test_task).unwrap();
        test_client.take_messages();
        let was_set_up = environment
          .take_stderr_messages()
          .iter()
          .any(|message| message == "Extracting zip for test-process-plugin");
        (test_client.take_shown_messages(), was_set_up)
      }

      fn assert_asked(shown: Vec<(MessageType, String, Vec<String>)>, reason: &str) {
        assert_eq!(shown.len(), 1, "{shown:?}");
        let (message_type, message, actions) = &shown[0];
        assert_eq!(*message_type, MessageType::WARNING);
        assert!(message.starts_with("dprint isn't formatting files with /dprint.json because "), "{message}");
        assert!(message.contains(reason), "{message}");
        assert_eq!(*actions, vec!["Allow", "Always Allow", "Deny", "Always Deny"]);
      }

      // a client that can't ask is told how to allow them
      let (shown, was_set_up) = run_session(&environment, Default::default(), None, [false, false]).await;
      assert!(!was_set_up);
      assert_eq!(shown.len(), 1, "{shown:?}");
      assert!(shown[0].1.contains("or when \"editor.allowProcessPlugins\" isn't false in"), "{shown:?}");
      assert_eq!(shown[0].2, Vec::<String>::new());

      // denying only asks once per session
      let (shown, was_set_up) = run_session(&environment, message_actions_capabilities(), Some("Deny"), [false, false]).await;
      assert!(!was_set_up);
      assert_asked(shown, "because its process plugins (https://plugins.dprint.dev/test-process.json@");

      // allowing uses it for the session
      let (shown, was_set_up) = run_session(&environment, message_actions_capabilities(), Some("Allow"), [false, true]).await;
      assert!(was_set_up);
      assert_asked(shown, "because its process plugins (https://plugins.dprint.dev/test-process.json@");

      // always denying doesn't ask in the next session
      let (shown, _) = run_session(&environment, message_actions_capabilities(), Some("Always Deny"), [false, false]).await;
      assert_asked(shown, "because its process plugins (https://plugins.dprint.dev/test-process.json@");
      assert_eq!(
        run_session(&environment, message_actions_capabilities(), None, [false, false]).await,
        (Vec::new(), false)
      );

      // always allowing replaces the decision and uses it in the next session
      environment.remove_file(get_editor_approvals_file_path(&environment).unwrap()).unwrap();
      let (shown, _) = run_session(&environment, message_actions_capabilities(), Some("Always Allow"), [false, true]).await;
      assert_asked(shown, "because its process plugins (https://plugins.dprint.dev/test-process.json@");
      assert_eq!(
        run_session(&environment, message_actions_capabilities(), None, [true, true]).await,
        (Vec::new(), false)
      );

      // asks again when its configuration changes
      environment
        .write_file(
          "/dprint.json",
          &environment
            .read_file("/dprint.json")
            .unwrap()
            .replacen('{', r#"{ "testProcessPlugin": { "ending": "changed" }, "#, 1),
        )
        .unwrap();
      let (shown, _) = run_session(&environment, message_actions_capabilities(), None, [false, false]).await;
      assert_asked(
        shown,
        "because the configuration of its process plugins (https://plugins.dprint.dev/test-process.json@",
      );
    });
  }

  #[test]
  fn should_use_process_plugins_of_global_config_without_asking_with_lsp() {
    // the global config file is the user's own
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .with_global_config(|c| {
        c.add_remote_process_plugin();
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn({
        let test_client = test_client.clone();
        async move {
          initialize_backend(
            &backend,
            InitializeParams {
              capabilities: message_actions_capabilities(),
              initialization_options: Some(serde_json::json!({ "useGlobalConfig": true })),
              ..Default::default()
            },
          )
          .await;
          let file_uri = Uri::from_str("file:///file.txt_ps").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_formatted_process".to_string()
            }])
          );
          assert_eq!(test_client.take_shown_messages(), Vec::new());
          backend.shutdown().await.unwrap();
        }
      });
      try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_not_notify_when_no_config_file_with_lsp_after_dismissed() {
    let environment = TestEnvironmentBuilder::new().build();

    environment.clone().run_in_runtime(async move {
      // each backend is a session and returns the messages it showed
      async fn run_session(environment: &TestEnvironment, workspace_folder: &str, selection: Option<&str>) -> Vec<(MessageType, String, Vec<String>)> {
        let (backend, recv_task, test_client) = setup_backend(environment.clone());
        if let Some(selection) = selection {
          test_client.set_message_action_selection(selection);
        }
        let workspace_folder = workspace_folder.to_string();
        let run_test_task = dprint_core::async_runtime::spawn(async move {
          initialize_backend(
            &backend,
            InitializeParams {
              capabilities: message_actions_capabilities(),
              workspace_folders: Some(vec![WorkspaceFolder {
                uri: Uri::from_str(&format!("file://{}", workspace_folder)).unwrap(),
                name: "folder".to_string(),
              }]),
              ..Default::default()
            },
          )
          .await;
          let file_uri = Uri::from_str(&format!("file://{}/file.txt", workspace_folder)).unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);
          // let the task that stores the user's selection run
          tokio::task::yield_now().await;
          backend.shutdown().await.unwrap();
        });
        try_join!(recv_task, run_test_task).unwrap();
        test_client.take_messages();
        environment.take_stderr_messages();
        test_client.take_shown_messages()
      }

      let actions = vec!["Don't show in this workspace".to_string(), "Don't show again".to_string()];
      let message = "No dprint configuration file found. Run \"dprint init\" in your project to create one.".to_string();
      let shown = vec![(MessageType::INFO, message, actions)];

      // shows again in the next session when the user doesn't select an action
      assert_eq!(run_session(&environment, "/project", None).await, shown);
      assert_eq!(run_session(&environment, "/project", Some("Don't show in this workspace")).await, shown);
      assert_eq!(run_session(&environment, "/project", None).await, Vec::new());
      // still shows in other workspaces
      assert_eq!(run_session(&environment, "/other", Some("Don't show again")).await, shown);
      assert_eq!(run_session(&environment, "/another", None).await, Vec::new());
    });
  }

  #[test]
  fn should_create_unversioned_workspace_edit_when_client_lacks_document_changes() {
    let uri = Uri::from_str("file:///file.txt").unwrap();
    let edits = vec![TextEdit {
      range: Range::new(Position::new(0, 7), Position::new(0, 7)),
      new_text: "_formatted".to_string(),
    }];
    assert!(!supports_document_changes(&ClientCapabilities::default()));
    assert_eq!(
      new_workspace_edit(&uri, Some(1), edits.clone(), false),
      WorkspaceEdit {
        changes: Some(HashMap::from([(uri, edits)])),
        ..Default::default()
      }
    );
  }

  #[test]
  fn should_format_with_global_config_commands_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.{txt,other}").add_excludes("ignored_file.txt");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn({
        let test_client = test_client.clone();
        async move {
          let result = initialize_backend(
            &backend,
            InitializeParams {
              capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                  workspace_edit: Some(WorkspaceEditClientCapabilities {
                    document_changes: Some(true),
                    ..Default::default()
                  }),
                  ..Default::default()
                }),
                ..Default::default()
              },
              ..Default::default()
            },
          )
          .await;
          assert_eq!(
            result.capabilities.execute_command_provider.unwrap().commands,
            vec!["dprint.formatWithGlobalConfig", "dprint.formatSelectionWithGlobalConfig"]
          );

          // formats the document
          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          execute_command(&backend, "dprint.formatWithGlobalConfig", vec![serde_json::json!(file_uri.as_str())])
            .await
            .unwrap();
          let edits = vec![TextEdit {
            range: Range::new(Position::new(0, 7), Position::new(0, 7)),
            new_text: "_formatted".to_string(),
          }];
          // has the version so the client rejects the edits when the document changed
          assert_eq!(
            test_client.take_applied_edits(),
            vec![WorkspaceEdit {
              document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                  uri: file_uri.clone(),
                  version: Some(0),
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
              }])),
              ..Default::default()
            }]
          );

          // formats a range
          let range = Range::new(Position::new(0, 0), Position::new(0, 7));
          execute_command(
            &backend,
            "dprint.formatSelectionWithGlobalConfig",
            vec![serde_json::json!(file_uri.as_str()), serde_json::to_value(range).unwrap()],
          )
          .await
          .unwrap();
          assert_eq!(test_client.take_applied_edits().len(), 1);
          assert_eq!(test_client.take_shown_messages(), Vec::new());

          // says nothing when already formatted
          let formatted_uri = Uri::from_str("file:///formatted.txt").unwrap();
          did_open!(backend, formatted_uri, "testing_formatted");
          execute_command(&backend, "dprint.formatWithGlobalConfig", vec![serde_json::json!(formatted_uri.as_str())])
            .await
            .unwrap();
          assert_eq!(test_client.take_shown_messages(), Vec::new());

          // says why the document wasn't formatted
          macro_rules! assert_not_formatted {
            ($uri:expr, $message_type:expr, $message:expr) => {
              let uri = Uri::from_str($uri).unwrap();
              did_open!(backend, uri, "testing");
              execute_command(&backend, "dprint.formatWithGlobalConfig", vec![serde_json::json!(uri.as_str())])
                .await
                .unwrap();
              assert_eq!(test_client.take_shown_messages(), vec![($message_type, $message.to_string(), Vec::new())]);
            };
          }
          assert_not_formatted!(
            "file:///ignored_file.txt",
            MessageType::INFO,
            "dprint did not format this document because the configuration file in use doesn't format it (ex. its \"includes\" and \"excludes\" don't match it)."
          );
          assert_not_formatted!(
            "file:///file.other",
            MessageType::INFO,
            "dprint did not format this document because no plugin in the configuration file in use handles it."
          );
          assert_not_formatted!(
            "other:///file.txt",
            MessageType::WARNING,
            "dprint failed to format this document. See the language server's log for details."
          );
          assert_eq!(test_client.take_applied_edits(), Vec::new());

          // invalid requests
          for (command, arguments) in [
            ("dprint.unknown", vec![serde_json::json!(file_uri.as_str())]),
            ("dprint.formatWithGlobalConfig", vec![]),
            ("dprint.formatWithGlobalConfig", vec![serde_json::json!(1)]),
            ("dprint.formatSelectionWithGlobalConfig", vec![serde_json::json!(file_uri.as_str())]),
          ] {
            assert!(execute_command(&backend, command, arguments).await.is_err(), "{}", command);
          }

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
      assert_eq!(
        test_client.take_messages().into_iter().skip(2).collect::<Vec<_>>(),
        vec![(
          MessageType::WARNING,
          "Cannot format document that is not a file, an untitled document or a cell of an open notebook: other:///file.txt".to_string()
        )]
      );
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_say_when_no_global_config_for_command_with_lsp() {
    let environment = TestEnvironmentBuilder::new().build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let run_test_task = dprint_core::async_runtime::spawn({
        let test_client = test_client.clone();
        async move {
          initialize_backend(&backend, Default::default()).await;
          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          execute_command(&backend, "dprint.formatWithGlobalConfig", vec![serde_json::json!(file_uri.as_str())])
            .await
            .unwrap();
          assert_eq!(
            test_client.take_shown_messages(),
            vec![(
              MessageType::INFO,
              "No dprint configuration file found. Run \"dprint init\" in your project to create one or \"dprint init --global\" to create a global one."
                .to_string(),
              Vec::new()
            )]
          );
          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_retry_failed_plugin_schema_download_with_lsp() {
    const SCHEMA_URL: &str = "https://plugins.dprint.dev/test/schema.json";
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/project/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .set_cwd("/project")
      .build();
    environment.set_fs_time(1_000);

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let file_uri = Uri::from_str("file:///project/dprint.json").unwrap();
          did_open!(backend, file_uri, "{\"test-plugin\":{}}");
          // gets the labels of the completions in the plugin's section
          let complete = || async {
            let result = backend
              .completion(
                CompletionParams {
                  text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                    position: Position::new(0, 16),
                  },
                  work_done_progress_params: Default::default(),
                  partial_result_params: Default::default(),
                  context: None,
                },
                CancellationToken::new(),
              )
              .await
              .unwrap();
            let Some(CompletionResponse::List(list)) = result else {
              panic!("expected completion items");
            };
            // nothing is being downloaded once a request was answered here
            assert!(!list.is_incomplete);
            list.items.into_iter().map(|item| item.label).collect::<Vec<_>>()
          };

          // the schema can't be downloaded, so only what's in the base schema
          let labels = complete().await;
          assert!(labels.contains(&"locked".to_string()), "{:?}", labels);
          assert!(!labels.contains(&"ending".to_string()), "{:?}", labels);
          assert_eq!(
            environment.take_stderr_messages(),
            vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
          );

          // not attempted again right away
          environment.add_remote_file_bytes(SCHEMA_URL, br#"{ "properties": { "ending": { "type": "string" } } }"#.to_vec());
          environment.set_fs_time(1_030);
          let labels = complete().await;
          assert!(!labels.contains(&"ending".to_string()), "{:?}", labels);

          // but it is later on
          environment.set_fs_time(1_090);
          let labels = complete().await;
          assert!(labels.contains(&"locked".to_string()), "{:?}", labels);
          assert!(labels.contains(&"ending".to_string()), "{:?}", labels);

          // and then the downloaded schema is kept
          environment.add_remote_file_bytes(SCHEMA_URL, br#"{ "properties": { "other": { "type": "string" } } }"#.to_vec());
          environment.set_fs_time(10_000);
          let labels = complete().await;
          assert!(labels.contains(&"ending".to_string()), "{:?}", labels);
          assert!(!labels.contains(&"other".to_string()), "{:?}", labels);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_mark_completions_incomplete_while_plugin_schema_downloads_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/project/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .set_cwd("/project")
      .build();
    environment.add_unresponsive_remote_file("https://plugins.dprint.dev/test/schema.json");

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let file_uri = Uri::from_str("file:///project/dprint.json").unwrap();
          did_open!(backend, file_uri, "{\"test-plugin\":{}}");
          // the first request gives up on the download and the second one
          // doesn't wait on it
          for _ in 0..2 {
            let result = backend
              .completion(
                CompletionParams {
                  text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: file_uri.clone() },
                    // in the plugin's section
                    position: Position::new(0, 16),
                  },
                  work_done_progress_params: Default::default(),
                  partial_result_params: Default::default(),
                  context: None,
                },
                CancellationToken::new(),
              )
              .await
              .unwrap();
            let Some(CompletionResponse::List(list)) = result else {
              panic!("expected completion items");
            };
            assert!(list.is_incomplete);
            assert!(list.items.iter().any(|item| item.label == "locked"));
          }
          assert_eq!(
            environment.take_stderr_messages(),
            vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
          );
          assert_eq!(environment.remote_file_request_count("https://plugins.dprint.dev/test/schema.json"), 1);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_warn_for_inherit_without_config_to_inherit_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      // not allowed in the global config file
      .with_global_config(|c| {
        c.set_inherit(true).add_remote_wasm_plugin();
      })
      // no config file in an ancestor directory
      .with_local_config("/project/dprint.json", |c| {
        c.set_inherit(true).add_remote_wasm_plugin();
      })
      .build();
    environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some("1"));

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        let test_client = test_client.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;
          test_client.take_messages();

          // the global config file is not inherited from, so
          // the config file is used on its own with a warning
          let file_uri = Uri::from_str("file:///project/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          let expected_edits = Some(vec![TextEdit {
            range: Range::new(Position::new(0, 7), Position::new(0, 7)),
            new_text: "_formatted".to_string(),
          }]);
          assert_format!(backend, file_uri, expected_edits.clone());
          let logs = environment.take_stderr_messages();
          let logs = logs.iter().filter(|log| !log.starts_with("Compiling ")).collect::<Vec<_>>();
          assert_eq!(logs.len(), 1, "{:?}", logs);
          assert!(
            logs[0].ends_with(concat!(
              "The 'inherit' property is ignored in /project/dprint.json because there is no ",
              "configuration file in an ancestor directory to inherit from."
            )),
            "{}",
            logs[0]
          );
          assert_eq!(test_client.take_messages(), vec![]);

          // warns each time the config is resolved
          assert_format!(backend, file_uri, expected_edits);
          assert_eq!(environment.take_stderr_messages().len(), 1);

          let file_uri = Uri::from_str("file:///scratch/file.txt").unwrap();
          let message = "The global configuration file cannot specify \"inherit\": true";
          did_open!(backend, file_uri, "testing");
          assert_format!(backend, file_uri, None);
          let logs = environment.take_stderr_messages();
          assert_eq!(logs.len(), 1, "{:?}", logs);
          assert!(logs[0].contains(message), "{}", logs[0]);
          let messages = test_client.take_messages();
          assert_eq!(messages.len(), 1, "{:?}", messages);
          assert_eq!(messages[0].0, MessageType::ERROR);
          assert!(messages[0].1.contains(message), "{}", messages[0].1);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
    });
  }

  #[test]
  fn should_format_with_lsp_using_config_override() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      // default config with a custom ending so we can tell which config is used
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_includes("**/*.txt")
          .add_config_section("test-plugin", r#"{"ending": "default"}"#);
      })
      // override config at a non-default path with a different ending
      .with_local_config("/custom/dprint.json", |c| {
        c.add_remote_wasm_plugin()
          .add_includes("**/*.txt")
          .add_config_section("test-plugin", r#"{"ending": "custom"}"#);
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      // pass the override config path
      let (backend, recv_task, test_client) = setup_backend_with_config(environment.clone(), Some(PathBuf::from("/custom/dprint.json")));
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          // should format using the overridden config (ending "custom"), not the default (ending "default")
          let file_uri = Uri::from_str("file:///custom/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 7), Position::new(0, 7)),
              new_text: "_custom".to_string()
            }])
          );

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_inherit_ancestor_config_with_lsp_using_config_override() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_includes("**/*.txt")
          .add_config_section("test-plugin", r#"{"ending": "default"}"#);
      })
      .with_local_config("/custom/dprint.json", |c| {
        c.set_inherit(true);
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend_with_config(environment.clone(), Some(PathBuf::from("/custom/dprint.json")));
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          // the specified config file inherits from the config file in its ancestor directory
          let file_uri = Uri::from_str("file:///custom/file.txt").unwrap();
          did_open!(backend, file_uri, "testing");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 7), Position::new(0, 7)),
              new_text: "_default".to_string()
            }])
          );

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
    });
  }

  #[test]
  fn should_format_untitled_documents_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_excludes("ignored-dir");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        async move {
          fn workspace_folder(uri: &str) -> WorkspaceFolder {
            WorkspaceFolder {
              uri: Uri::from_str(uri).unwrap(),
              name: "folder".to_string(),
            }
          }

          let result = backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              capabilities: serde_json::from_value(serde_json::json!({
                "textDocument": {
                  "synchronization": { "dynamicRegistration": true },
                  "formatting": { "dynamicRegistration": true },
                  "rangeFormatting": { "dynamicRegistration": true },
                }
              }))
              .unwrap(),
              workspace_folders: Some(vec![workspace_folder("file:///workspace")]),
              ..Default::default()
            })
            .await
            .unwrap();
          // the client tells the server when the workspace folders change
          assert_eq!(
            serde_json::to_value(result.capabilities.workspace).unwrap(),
            serde_json::json!({ "workspaceFolders": { "supported": true, "changeNotifications": true } })
          );
          backend.initialized(InitializedParams {}).await;

          macro_rules! did_open_untitled {
            ($uri:expr, $language_id:expr) => {
              backend
                .did_open(DidOpenTextDocumentParams {
                  text_document: TextDocumentItem {
                    uri: $uri.clone(),
                    language_id: $language_id.to_string(),
                    version: 0,
                    text: "text".to_string(),
                  },
                })
                .await;
            };
          }

          // formats as a file of the language in the workspace folder
          let file_uri = Uri::from_str("untitled:Untitled-1").unwrap();
          did_open_untitled!(file_uri, "txt");
          assert_format!(
            backend,
            file_uri,
            Some(vec![TextEdit {
              range: Range::new(Position::new(0, 4), Position::new(0, 4)),
              new_text: "_formatted".to_string()
            }])
          );

          // language without a plugin
          let other_file_uri = Uri::from_str("untitled:Untitled-2").unwrap();
          did_open_untitled!(other_file_uri, "python");
          assert_format!(backend, other_file_uri, None);

          // uses the first workspace folder after they change, which is excluded
          backend
            .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
              event: WorkspaceFoldersChangeEvent {
                added: vec![workspace_folder("file:///ignored-dir")],
                removed: vec![workspace_folder("file:///workspace")],
              },
            })
            .await;
          assert_format!(backend, file_uri, None);

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();

      assert_eq!(
        test_client.take_messages(),
        vec![
          (
            MessageType::INFO,
            format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
          ),
          (MessageType::INFO, "Server ready.".to_string())
        ]
      );
      // the server has the client send it untitled documents
      assert_eq!(
        test_client.take_registered_methods(),
        vec![
          "textDocument/didOpen",
          "textDocument/didChange",
          "textDocument/didClose",
          "textDocument/formatting",
          "textDocument/rangeFormatting",
        ]
      );
    });
  }

  #[test]
  fn should_only_provide_completion_and_hover_for_config_files_with_lsp() {
    // (client capabilities, has static completion, has static hover, registered methods)
    let cases: Vec<(serde_json::Value, bool, bool, Vec<&str>)> = vec![
      (
        serde_json::json!({
          "textDocument": {
            "completion": { "dynamicRegistration": true },
            "hover": { "dynamicRegistration": true },
          }
        }),
        false,
        false,
        vec!["textDocument/completion", "textDocument/hover"],
      ),
      // a client that can only register one of them
      (
        serde_json::json!({
          "textDocument": {
            "completion": { "dynamicRegistration": false },
            "hover": { "dynamicRegistration": true },
          }
        }),
        true,
        false,
        vec!["textDocument/hover"],
      ),
      (
        serde_json::json!({
          "textDocument": {
            "completion": { "dynamicRegistration": true },
          }
        }),
        false,
        true,
        vec!["textDocument/completion"],
      ),
      // a client that can't register them has them for every document
      (serde_json::json!({}), true, true, vec![]),
    ];

    for (capabilities, has_static_completion, has_static_hover, registered_methods) in cases {
      let environment = TestEnvironmentBuilder::new().build();
      environment.clone().run_in_runtime(async move {
        let (backend, recv_task, test_client) = setup_backend(environment.clone());
        let result = backend
          .initialize(InitializeParams {
            process_id: Some(std::process::id()),
            capabilities: serde_json::from_value(capabilities.clone()).unwrap(),
            ..Default::default()
          })
          .await
          .unwrap();
        assert_eq!(
          serde_json::to_value(&result.capabilities.completion_provider).unwrap(),
          if has_static_completion {
            serde_json::json!({ "triggerCharacters": ["\"", ":"] })
          } else {
            serde_json::Value::Null
          },
          "completion for {}",
          capabilities
        );
        assert_eq!(
          serde_json::to_value(&result.capabilities.hover_provider).unwrap(),
          if has_static_hover { serde_json::json!(true) } else { serde_json::Value::Null },
          "hover for {}",
          capabilities
        );
        // nothing is registered until the client is initialized
        assert_eq!(test_client.take_registered_methods(), Vec::<String>::new());
        backend.initialized(InitializedParams {}).await;
        backend.shutdown().await.unwrap();
        recv_task.await.unwrap();

        assert_eq!(
          test_client.take_messages(),
          vec![
            (
              MessageType::INFO,
              format!("dprint {} ({}-{})", environment.cli_version(), environment.os(), environment.cpu_arch())
            ),
            (MessageType::INFO, "Server ready.".to_string())
          ]
        );
        assert_eq!(test_client.take_registered_methods(), registered_methods, "registrations for {}", capabilities);
      });
    }
  }

  #[test]
  fn should_log_format_failures_to_client_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        let test_client = test_client.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;
          test_client.take_messages();

          macro_rules! assert_range_format {
            ($uri:expr, $range:expr, $expected:expr) => {
              let result = backend
                .range_formatting(
                  DocumentRangeFormattingParams {
                    text_document: TextDocumentIdentifier { uri: $uri.clone() },
                    range: $range,
                    options: Default::default(),
                    work_done_progress_params: Default::default(),
                  },
                  CancellationToken::new(),
                )
                .await;
              assert_eq!(result.unwrap(), $expected);
            };
          }
          macro_rules! assert_logged {
            ($message_type:expr, $message:expr) => {
              assert_eq!(environment.take_stderr_messages(), vec![$message.to_string()]);
              assert_eq!(test_client.take_messages(), vec![($message_type, $message.to_string())]);
            };
          }

          // the plugin failing to format the file, which is not a json-rpc
          // error because that's what happens for a file with a syntax error
          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "should_error");
          assert_format!(backend, file_uri, None);
          assert_logged!(MessageType::ERROR, "Failed formatting 'file:///file.txt': Did error.");

          // range with its start after its end
          assert_range_format!(file_uri, Range::new(Position::new(0, 2), Position::new(0, 1)), None);
          assert_logged!(
            MessageType::WARNING,
            "Invalid range for 'file:///file.txt'. The start of the range was after its end."
          );

          // document that's not open
          let not_open_uri = Uri::from_str("file:///not_open.txt").unwrap();
          assert_format!(backend, not_open_uri, None);
          assert_logged!(MessageType::WARNING, "Missing document: file:///not_open.txt");
          let valid_range = Range::new(Position::new(0, 0), Position::new(0, 1));
          assert_range_format!(not_open_uri, valid_range, None);
          assert_logged!(MessageType::WARNING, "Missing document: file:///not_open.txt");
          let not_open_uri = Uri::from_str("untitled:Untitled-1").unwrap();
          assert_format!(backend, not_open_uri, None);
          assert_logged!(MessageType::WARNING, "Missing document: untitled:Untitled-1");

          // document that's not on the file system
          let other_uri = Uri::from_str("other:/file.txt").unwrap();
          did_open!(backend, other_uri, "text");
          assert_format!(backend, other_uri, None);
          assert_logged!(
            MessageType::WARNING,
            "Cannot format document that is not a file, an untitled document or a cell of an open notebook: other:/file.txt"
          );
          assert_range_format!(other_uri, valid_range, None);
          assert_logged!(
            MessageType::WARNING,
            "Cannot format document that is not a file, an untitled document or a cell of an open notebook: other:/file.txt"
          );

          // path without a parent directory
          let root_uri = Uri::from_str("file:///").unwrap();
          did_open!(backend, root_uri, "text");
          assert_format!(backend, root_uri, None);
          assert_logged!(MessageType::WARNING, "Cannot format non-file path: /");
          assert_range_format!(root_uri, valid_range, None);
          assert_logged!(MessageType::WARNING, "Cannot format non-file path: /");

          // untitled document in a language without a known file extension
          let untitled_uri = Uri::from_str("untitled:Untitled-2").unwrap();
          backend
            .did_open(DidOpenTextDocumentParams {
              text_document: TextDocumentItem {
                uri: untitled_uri.clone(),
                language_id: "some language".to_string(),
                version: 0,
                text: "text".to_string(),
              },
            })
            .await;
          assert_format!(backend, untitled_uri, None);
          assert_logged!(
            MessageType::ERROR,
            "Failed formatting 'untitled:Untitled-2': Could not determine a file path to format the document with language: some language"
          );

          // config file that fails to resolve
          environment.write_file("/dprint.json", "{").unwrap();
          did_close!(backend, file_uri);
          did_open!(backend, file_uri, "text");
          assert_format!(backend, file_uri, None);
          assert_logged!(
            MessageType::ERROR,
            "Failed formatting 'file:///file.txt': Error deserializing. Unterminated object on line 1 column 2\n    at /dprint.json"
          );

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
    });
  }

  #[test]
  fn should_log_plugin_config_diagnostics_to_client_with_lsp() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "non-existent": 1 }"#)
          .add_includes("**/*.txt");
      })
      .initialize()
      .build();

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        let test_client = test_client.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;
          test_client.take_messages();

          // stderr only has the text of the diagnostics the first time the
          // plugin is used, but each failure says what they are
          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "text");
          let failure_message = concat!(
            "Failed formatting 'file:///file.txt': dprint did not format the file because of errors in the configuration of /dprint.json:
",
            "[test-plugin]: Unknown property in configuration (non-existent)"
          );
          let client_message = (MessageType::ERROR, failure_message.to_string());
          assert_format!(backend, file_uri, None);
          assert_eq!(
            environment.take_stderr_messages(),
            vec![
              "[test-plugin]: Unknown property in configuration (non-existent)",
              "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s).",
              failure_message,
            ]
          );
          assert_eq!(test_client.take_messages(), vec![client_message.clone()]);
          assert_format!(backend, file_uri, None);
          assert_eq!(
            environment.take_stderr_messages(),
            vec![failure_message]
          );
          assert_eq!(test_client.take_messages(), vec![client_message]);

          // diagnostics in a plugin's overrides
          let mut config_file = TestConfigFileBuilder::new(environment.clone());
          config_file.add_remote_wasm_plugin().add_config_section(
            "test-plugin",
            r#"{
              "overrides": {
                "files": "**/other.txt",
                "unknownProperty": true
              }
            }"#,
          );
          environment.mk_dir_all("/overrides").unwrap();
          environment.write_file("/overrides/dprint.json", &config_file.to_string()).unwrap();
          let file_uri = Uri::from_str("file:///overrides/file.txt").unwrap();
          did_open!(backend, file_uri, "text");
          assert_format!(backend, file_uri, None);
          assert_eq!(
            environment.take_stderr_messages(),
            vec![
              "[test-plugin]: Unknown property in configuration (unknownProperty)",
              "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s).",
              concat!(
                "Failed formatting 'file:///overrides/file.txt': dprint did not format the file because of errors in the configuration of /overrides/dprint.json:
",
                "[test-plugin]: Unknown property in configuration (unknownProperty)"
              ),
            ]
          );
          assert_eq!(
            test_client.take_messages(),
            vec![(
              MessageType::ERROR,
              concat!(
                "Failed formatting 'file:///overrides/file.txt': dprint did not format the file because of errors in the configuration of /overrides/dprint.json:\n",
                "[test-plugin]: Unknown property in configuration (unknownProperty)"
              )
              .to_string()
            )]
          );

          backend.shutdown().await.unwrap();
        }
      });

      try_join!(recv_task, run_test_task).unwrap();
    });
  }

  #[test]
  fn should_ensure_stable_format_with_lsp() {
    // formats once by default
    let (edits, stderr_messages) = format_unstable_text_with_lsp(None);
    assert_eq!(
      edits,
      Some(vec![
        TextEdit {
          range: Range::new(Position::new(0, 13), Position::new(0, 14)),
          new_text: "false_fo".to_string()
        },
        TextEdit {
          range: Range::new(Position::new(0, 15), Position::new(0, 16)),
          new_text: "matt".to_string()
        },
        TextEdit {
          range: Range::new(Position::new(0, 17), Position::new(0, 17)),
          new_text: "d".to_string()
        },
      ])
    );
    assert_eq!(stderr_messages, Vec::<String>::new());

    // formats again until the output is stable, like `dprint fmt`, when opted in
    let (edits, stderr_messages) = format_unstable_text_with_lsp(Some("true"));
    assert_eq!(edits, None);
    assert_eq!(
      stderr_messages,
      vec![concat!(
        "Failed formatting 'file:///file.txt': Formatting not stable. Bailed after 5 tries. ",
        "This indicates a bug in the plugin where it formats the file differently each time."
      )],
    );
  }

  fn format_unstable_text_with_lsp(stable_format_env_var: Option<&str>) -> (Option<Vec<TextEdit>>, Vec<String>) {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt");
      })
      .initialize()
      .build();
    environment.set_env_var("DPRINT_EDITOR_STABLE_FORMAT", stable_format_env_var);

    environment.clone().run_in_runtime(async move {
      let (backend, recv_task, test_client) = setup_backend(environment.clone());
      let backend = Rc::new(backend);
      let run_test_task = dprint_core::async_runtime::spawn({
        let environment = environment.clone();
        async move {
          backend
            .initialize(InitializeParams {
              process_id: Some(std::process::id()),
              ..Default::default()
            })
            .await
            .unwrap();
          backend.initialized(InitializedParams {}).await;

          let file_uri = Uri::from_str("file:///file.txt").unwrap();
          did_open!(backend, file_uri, "unstable_fmt_true");
          let edits = backend
            .formatting(
              DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri: file_uri },
                options: Default::default(),
                work_done_progress_params: Default::default(),
              },
              CancellationToken::new(),
            )
            .await
            .unwrap();
          let stderr_messages = environment.take_stderr_messages();

          backend.shutdown().await.unwrap();
          (edits, stderr_messages)
        }
      });

      let (_, result) = try_join!(recv_task, run_test_task).unwrap();
      test_client.take_messages();
      result
    })
  }

  #[test]
  fn exit_code_for_shutdown_request() {
    // the message handler says a shutdown request was received
    let environment = TestEnvironmentBuilder::new().build();
    let shutdown_received = environment.clone().run_in_runtime(async move {
      let (backend, recv_task, _test_client) = setup_backend(environment.clone());
      backend.shutdown().await.unwrap();
      // a second one after the message handler stopped should not hang
      backend.shutdown().await.unwrap();
      recv_task.await.unwrap()
    });
    assert!(shutdown_received);
    assert_eq!(get_exit_code(shutdown_received), 0);

    // and that one wasn't when the client exits or disconnects without one,
    // which drops the backend
    let environment = TestEnvironmentBuilder::new().build();
    let shutdown_received = environment.clone().run_in_runtime(async move {
      let (backend, recv_task, _test_client) = setup_backend(environment.clone());
      drop(backend);
      recv_task.await.unwrap()
    });
    assert!(!shutdown_received);
    assert_eq!(get_exit_code(shutdown_received), 1);
  }

  async fn initialize_backend(backend: &Backend<TestEnvironment>, params: InitializeParams) -> InitializeResult {
    let result = backend
      .initialize(InitializeParams {
        process_id: Some(std::process::id()),
        ..params
      })
      .await
      .unwrap();
    backend.initialized(InitializedParams {}).await;
    result
  }

  async fn execute_command(backend: &Backend<TestEnvironment>, command: &str, arguments: Vec<serde_json::Value>) -> LspResult<Option<serde_json::Value>> {
    backend
      .execute_command(
        ExecuteCommandParams {
          command: command.to_string(),
          arguments,
          work_done_progress_params: Default::default(),
        },
        CancellationToken::new(),
      )
      .await
  }

  fn ensure_global_config_dir(environment: &TestEnvironment) {
    // macOS only uses this directory when it's already there
    let file_path = get_settings_file_path(environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
  }

  /// Requires the user to approve running process plugins.
  fn disallow_process_plugins(environment: &TestEnvironment) {
    ensure_global_config_dir(environment);
    let file_path = get_settings_file_path(environment).unwrap();
    environment.write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": false } }"#).unwrap();
  }

  /// The capabilities of a client that shows the actions of a message.
  fn message_actions_capabilities() -> ClientCapabilities {
    ClientCapabilities {
      window: Some(WindowClientCapabilities {
        // the actions don't have additional properties, so this isn't necessary
        show_message: Some(ShowMessageRequestClientCapabilities { message_action_item: None }),
        ..Default::default()
      }),
      ..Default::default()
    }
  }

  fn setup_backend(environment: TestEnvironment) -> (Backend<TestEnvironment>, JoinHandle<bool>, Arc<TestClient>) {
    setup_backend_with_config(environment, None)
  }

  fn setup_backend_with_config(
    environment: TestEnvironment,
    config_override: Option<PathBuf>,
  ) -> (Backend<TestEnvironment>, JoinHandle<bool>, Arc<TestClient>) {
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache, Default::default()));
    let (tx, rx) = mpsc::unbounded_channel();
    let session_permissions = Arc::new(SessionPermissions::default());
    let recv_task = start_message_handler(
      &environment,
      &plugin_resolver,
      config_override,
      RemoteCacheMode::Use,
      session_permissions.clone(),
      rx,
    );
    let test_client = Arc::new(TestClient::default());
    (Backend::new(ClientWrapper::new(test_client.clone()), environment, tx), recv_task, test_client)
  }

  #[derive(Debug, Default)]
  struct TestClient {
    logged_messages: Mutex<Vec<(MessageType, String)>>,
    registrations: Mutex<Vec<Registration>>,
    /// The messages shown to the user along with the titles of their actions.
    shown_messages: Mutex<Vec<(MessageType, String, Vec<String>)>>,
    /// The title of the action to select when a message with actions is shown.
    message_action_selection: Mutex<Option<String>>,
    applied_edits: Mutex<Vec<WorkspaceEdit>>,
    published_diagnostics: Mutex<Vec<(Uri, Vec<Diagnostic>)>>,
    config_changes: Mutex<Vec<Uri>>,
    /// The progress that was sent with whether the client was asked to create it.
    progress: Mutex<Vec<(NumberOrString, WorkDoneProgress)>>,
  }

  impl Drop for TestClient {
    fn drop(&mut self) {
      // If this panics that means the logged messages weren't inspected for a test.
      if !std::thread::panicking() {
        let logged_messages = self.logged_messages.lock().clone();
        assert_eq!(
          logged_messages,
          Vec::<(MessageType, String)>::new(),
          "should not have logged messages left on drop"
        );
      }
    }
  }

  impl TestClient {
    pub fn take_messages(&self) -> Vec<(MessageType, String)> {
      self.logged_messages.lock().drain(..).collect()
    }

    pub fn take_registered_methods(&self) -> Vec<String> {
      self.registrations.lock().drain(..).map(|r| r.method).collect()
    }

    pub fn take_shown_messages(&self) -> Vec<(MessageType, String, Vec<String>)> {
      self.shown_messages.lock().drain(..).collect()
    }

    pub fn set_message_action_selection(&self, title: &str) {
      *self.message_action_selection.lock() = Some(title.to_string());
    }

    pub fn take_applied_edits(&self) -> Vec<WorkspaceEdit> {
      self.applied_edits.lock().drain(..).collect()
    }
  }

  impl ClientTrait for TestClient {
    fn log(&self, message_type: MessageType, message: String) {
      self.logged_messages.lock().push((message_type, message));
    }

    fn register_capabilities(&self, registrations: Vec<Registration>) {
      self.registrations.lock().extend(registrations);
    }

    fn show_message(&self, message_type: MessageType, message: String) {
      self.shown_messages.lock().push((message_type, message, Vec::new()));
    }

    fn show_message_request(
      &self,
      message_type: MessageType,
      message: String,
      actions: Vec<MessageActionItem>,
    ) -> LocalBoxFuture<'static, Option<MessageActionItem>> {
      let selection = self.message_action_selection.lock().clone();
      let selection = actions.iter().find(|action| Some(&action.title) == selection.as_ref()).cloned();
      let titles = actions.into_iter().map(|action| action.title).collect();
      self.shown_messages.lock().push((message_type, message, titles));
      async move { selection }.boxed_local()
    }

    fn apply_edit(&self, edit: WorkspaceEdit) -> LocalBoxFuture<'static, Result<()>> {
      self.applied_edits.lock().push(edit);
      async move { Ok(()) }.boxed_local()
    }

    fn publish_diagnostics(&self, uri: Uri, diagnostics: Vec<Diagnostic>) {
      self.published_diagnostics.lock().push((uri, diagnostics));
    }

    fn send_config_changed(&self, uri: Uri) {
      self.config_changes.lock().push(uri);
    }

    fn send_log(&self, message_type: MessageType, message: String) -> LocalBoxFuture<'static, ()> {
      self.logged_messages.lock().push((message_type, message));
      async {}.boxed_local()
    }

    fn create_work_done_progress(&self, _token: NumberOrString) -> LocalBoxFuture<'static, bool> {
      async { true }.boxed_local()
    }

    fn send_progress(&self, token: NumberOrString, progress: WorkDoneProgress) -> LocalBoxFuture<'static, ()> {
      self.progress.lock().push((token, progress));
      async {}.boxed_local()
    }
  }
}
