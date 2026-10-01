use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use deno_tower_lsp::CancellationToken;
use deno_tower_lsp::LanguageServer;
use deno_tower_lsp::LspService;
use deno_tower_lsp::Server;
use deno_tower_lsp::jsonrpc::Result as LspResult;
use deno_tower_lsp::lsp_types::CompletionItem;
use deno_tower_lsp::lsp_types::CompletionOptions;
use deno_tower_lsp::lsp_types::CompletionParams;
use deno_tower_lsp::lsp_types::CompletionResponse;
use deno_tower_lsp::lsp_types::DidChangeNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidChangeTextDocumentParams;
use deno_tower_lsp::lsp_types::DidCloseNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidCloseTextDocumentParams;
use deno_tower_lsp::lsp_types::DidOpenNotebookDocumentParams;
use deno_tower_lsp::lsp_types::DidOpenTextDocumentParams;
use deno_tower_lsp::lsp_types::DocumentFormattingParams;
use deno_tower_lsp::lsp_types::DocumentRangeFormattingParams;
use deno_tower_lsp::lsp_types::Hover;
use deno_tower_lsp::lsp_types::HoverParams;
use deno_tower_lsp::lsp_types::HoverProviderCapability;
use deno_tower_lsp::lsp_types::InitializeParams;
use deno_tower_lsp::lsp_types::InitializeResult;
use deno_tower_lsp::lsp_types::InitializedParams;
use deno_tower_lsp::lsp_types::OneOf;
use deno_tower_lsp::lsp_types::Position;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::ServerCapabilities;
use deno_tower_lsp::lsp_types::ServerInfo;
use deno_tower_lsp::lsp_types::TextDocumentSyncCapability;
use deno_tower_lsp::lsp_types::TextDocumentSyncKind;
use deno_tower_lsp::lsp_types::TextDocumentSyncOptions;
use deno_tower_lsp::lsp_types::TextEdit;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::async_runtime::JoinHandle;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::process::start_parent_process_checker_task;
use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::try_join;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::plugins::PluginResolver;

use self::client::ClientWrapper;
use self::config::LspPluginsScopeContainer;
use self::config_completion::ConfigCompletions;
use self::config_completion::is_config_uri;
use self::documents::Documents;
use self::notebook::get_notebook_cell_file_path;
use self::notebook::get_notebook_cell_format_registrations;
use self::notebook::get_notebook_document_sync_options;
use self::notebook::trim_formatted_cell_text;
use self::text::LineIndex;
use self::text::get_edits;
use self::text::normalize_to_source_line_endings;

mod client;
mod config;
mod config_completion;
mod documents;
mod notebook;
mod text;

// deno_tower_lsp will drop the future on cancellation,
// so use this to cancel the containing token on drop.
struct DropToken {
  completed: bool,
  token: Arc<CancellationToken>,
}

impl DropToken {
  pub fn new(token: Arc<CancellationToken>) -> Self {
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

struct PendingTokenGuard {
  id: u16,
  tokens: Rc<RefCell<HashMap<u16, Arc<CancellationToken>>>>,
}

impl Drop for PendingTokenGuard {
  fn drop(&mut self) {
    self.tokens.borrow_mut().remove(&self.id);
  }
}

#[derive(Default)]
struct PendingTokens {
  next_id: u16,
  tokens: Rc<RefCell<HashMap<u16, Arc<CancellationToken>>>>,
}

impl PendingTokens {
  pub fn insert(&mut self, token: Arc<CancellationToken>) -> PendingTokenGuard {
    let id = self.next_id();
    self.tokens.borrow_mut().insert(id, token);
    PendingTokenGuard {
      id,
      tokens: self.tokens.clone(),
    }
  }

  pub fn cancel_all(&mut self) {
    let mut pending_tokens = self.tokens.borrow_mut();
    for token in pending_tokens.values() {
      token.cancel();
    }
    pending_tokens.clear();
    self.next_id = 0;
  }

  pub fn next_id(&mut self) -> u16 {
    if self.next_id == u16::MAX {
      self.next_id = 0;
    }
    let id = self.next_id;
    self.next_id += 1;
    id
  }
}

struct EditorFormatRequest {
  /// The file path to format the document's text as, which differs from the
  /// document's for documents that aren't on the file system (ex. notebook cells).
  pub file_path: PathBuf,
  /// The notebook file's path when the document is a notebook cell.
  pub notebook_path: Option<PathBuf>,
  pub file_text: String,
  pub maybe_line_index: Option<LineIndex>,
  pub range: FormatRange,
  pub token: Arc<CancellationToken>,
}

struct ConfigEditorRequest {
  pub file_path: PathBuf,
  pub file_text: String,
  pub position: Position,
}

enum ChannelMessage {
  Format(EditorFormatRequest, oneshot::Sender<Result<Option<Vec<TextEdit>>>>),
  Completion(ConfigEditorRequest, oneshot::Sender<Option<Vec<CompletionItem>>>),
  Hover(ConfigEditorRequest, oneshot::Sender<Option<Hover>>),
  Shutdown(oneshot::Sender<()>),
  /// This message is used for testing.
  #[cfg(test)]
  HasPending(oneshot::Sender<bool>),
}

async fn handle_format_request<TEnvironment: Environment>(
  mut request: EditorFormatRequest,
  scope_container: Rc<LspPluginsScopeContainer<TEnvironment>>,
  ensure_stable_format: EnsureStableFormat,
  environment: &TEnvironment,
) -> Result<Option<Vec<TextEdit>>> {
  let Some(parent_dir) = request.file_path.parent() else {
    log_warn!(environment, "Cannot format non-file path: {}", request.file_path.display());
    return Ok(None);
  };
  if request.token.is_cancelled() {
    return Ok(None);
  }
  let Some(scope) = scope_container.resolve_by_path(parent_dir).await? else {
    log_stderr_info!(environment, "Path did not have a dprint config file: {}", request.file_path.display());
    return Ok(None);
  };
  if request.token.is_cancelled() {
    return Ok(None);
  }
  // canonicalize the paths
  request.file_path = canonicalize_path(environment, request.file_path);
  request.notebook_path = request.notebook_path.map(|path| canonicalize_path(environment, path));

  let can_format = match &request.notebook_path {
    // the cli formats a notebook's cells when a plugin (the jupyter plugin) formats
    // the notebook, so only format a cell when the notebook would be formatted
    Some(notebook_path) => {
      !scope.plugin_name_maps.get_plugin_names_from_file_path(notebook_path).into_names().is_empty() && scope.can_format_for_editor(notebook_path, None)
    }
    None => scope.can_format_for_editor(&request.file_path, Some(request.file_text.as_bytes())),
  };
  if !can_format {
    log_debug!(
      environment,
      "Excluded file: {}",
      request.notebook_path.as_ref().unwrap_or(&request.file_path).display()
    );
    return Ok(None);
  }

  // the range is given to the plugin, but is needed after for a notebook cell
  let maybe_cell_range = request.notebook_path.as_ref().map(|_| request.range.clone());
  let Some(result) = scope
    .format_stable(
      HostFormatRequest {
        file_path: request.file_path,
        file_bytes: request.file_text.as_bytes().to_vec(),
        range: request.range,
        override_config: Default::default(),
        token: request.token,
      },
      ensure_stable_format,
    )
    .await?
  else {
    return Ok(None);
  };
  dprint_core::async_runtime::spawn_blocking(move || {
    let new_text = String::from_utf8(result).context("Failed converting formatted text to utf-8.")?;
    // the editor owns the document's line endings, so match them rather than
    // imposing the plugin's configured newline kind (see #965)
    let new_text = normalize_to_source_line_endings(&request.file_text, new_text);
    let new_text = match maybe_cell_range {
      Some(range) => {
        let new_text = trim_formatted_cell_text(&request.file_text, new_text, &range);
        if new_text == request.file_text {
          return Ok(None);
        }
        new_text
      }
      None => new_text,
    };
    let line_index = request.maybe_line_index.unwrap_or_else(|| LineIndex::new(&request.file_text));
    Ok(Some(get_edits(&request.file_text, &new_text, &line_index)))
  })
  .await?
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
  let recv_task = start_message_handler(environment, plugin_resolver, config_path, rx);

  let environment = environment.clone();
  let lsp_task = dprint_core::async_runtime::spawn(async move {
    let (service, socket, pending) = LspService::new(|client| {
      let client = ClientWrapper::new(Arc::new(client));
      Backend::new(client.clone(), environment.clone(), tx)
    });
    Server::new(stdin, stdout, socket, pending).serve(service).await;
  });

  try_join!(recv_task, lsp_task)?;

  Ok(())
}

fn start_message_handler<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  config_override: Option<PathBuf>,
  mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
) -> JoinHandle<()> {
  // tower_lsp required Backend to implement Send and Sync, but
  // we use a single threaded runtime. So spawn some tasks and
  // communicate over a channel.
  // todo: deno_tower_lsp doesn't require this, so the channel could be removed
  let max_cores = environment.max_threads();
  let concurrency_limiter = Rc::new(Semaphore::new(std::cmp::max(1, max_cores - 1)));
  let ensure_stable_format = EnsureStableFormat::for_editor(environment);
  let environment = environment.clone();
  let scope_container = Rc::new(LspPluginsScopeContainer::new(environment.clone(), plugin_resolver.clone(), config_override));
  let config_completions = Rc::new(ConfigCompletions::new(environment.clone(), scope_container.clone()));
  dprint_core::async_runtime::spawn(async move {
    let mut pending_tokens = PendingTokens::default();
    while let Some(message) = rx.recv().await {
      match message {
        ChannelMessage::Format(request, sender) => {
          let token_guard = pending_tokens.insert(request.token.clone());
          let concurrency_limiter = concurrency_limiter.clone();
          let scope_container = scope_container.clone();
          let environment = environment.clone();
          dprint_core::async_runtime::spawn(async move {
            let _permit = concurrency_limiter.acquire().await;
            let result = handle_format_request(request, scope_container, ensure_stable_format, &environment).await;
            let _ = sender.send(result);
            drop(token_guard); // remove the token from the pending tokens
          });
        }
        ChannelMessage::Completion(request, sender) => {
          let config_completions = config_completions.clone();
          dprint_core::async_runtime::spawn(async move {
            let result = config_completions.completions(&request.file_path, &request.file_text, request.position).await;
            let _ = sender.send(result);
          });
        }
        ChannelMessage::Hover(request, sender) => {
          let config_completions = config_completions.clone();
          dprint_core::async_runtime::spawn(async move {
            let result = config_completions.hover(&request.file_path, &request.file_text, request.position).await;
            let _ = sender.send(result);
          });
        }
        ChannelMessage::Shutdown(sender) => {
          pending_tokens.cancel_all();
          scope_container.shutdown().await;
          let _ = sender.send(());
          break; // exit
        }
        #[cfg(test)]
        ChannelMessage::HasPending(sender) => {
          let is_empty = pending_tokens.tokens.borrow().is_empty();
          let _ = sender.send(!is_empty);
        }
      }
    }
  })
}

struct State<TEnvironment: Environment> {
  documents: Documents<TEnvironment>,
  /// Registrations to send to the client once it says it's initialized.
  pending_registrations: Vec<Registration>,
}

struct Backend<TEnvironment: Environment> {
  client: ClientWrapper,
  environment: TEnvironment,
  sender: mpsc::UnboundedSender<ChannelMessage>,
  state: Mutex<State<TEnvironment>>,
}

impl<TEnvironment: Environment> Backend<TEnvironment> {
  pub fn new(client: ClientWrapper, environment: TEnvironment, sender: mpsc::UnboundedSender<ChannelMessage>) -> Self {
    Backend {
      client,
      environment: environment.clone(),
      sender,
      state: Mutex::new(State {
        documents: Documents::new(environment),
        pending_registrations: Vec::new(),
      }),
    }
  }

  async fn send_format_request(&self, uri: &Uri, request: EditorFormatRequest) -> LspResult<Option<Vec<TextEdit>>> {
    let start_time = std::time::Instant::now();
    log_debug!(self.environment, "Received format request for {}", uri.as_str());
    let mut drop_token = DropToken::new(request.token.clone());
    let result = self.send_format_request_inner(request).await;
    drop_token.completed();
    let result = match result {
      Ok(value) => Ok(value),
      Err(err) => {
        log_error!(self.environment, "Failed formatting '{}': {:#}", uri.as_str(), err);
        Ok(None)
      }
    };
    log_debug!(
      self.environment,
      "Finished format request for {} in {}ms",
      uri.as_str(),
      start_time.elapsed().as_millis()
    );
    result
  }

  /// Resolves the file path to format the document as along with the path of
  /// its notebook when the document is a notebook cell.
  fn resolve_format_paths(&self, uri: &Uri) -> Option<(PathBuf, Option<PathBuf>)> {
    let Some((notebook_uri, language_id)) = self.state.lock().documents.get_notebook_cell(uri) else {
      return Some((uri_to_file_path(uri)?, None));
    };
    // the cli only formats notebooks on the file system
    let notebook_path = uri_to_file_path(&notebook_uri)?;
    let Some(file_path) = get_notebook_cell_file_path(&notebook_path, &language_id) else {
      log_debug!(
        self.environment,
        "Could not determine a file path to format the notebook cell with language: {}",
        language_id
      );
      return None;
    };
    Some((file_path, Some(notebook_path)))
  }

  async fn send_format_request_inner(&self, request: EditorFormatRequest) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    self.sender.send(ChannelMessage::Format(request, sender))?;
    receiver.await?
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
    self.state.lock().pending_registrations = get_notebook_cell_format_registrations(&params.capabilities);

    Ok(InitializeResult {
      server_info: Some(ServerInfo {
        name: "dprint".to_string(),
        version: Some(self.environment.cli_version()),
      }),
      capabilities: ServerCapabilities {
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
        completion_provider: Some(CompletionOptions {
          // `"` opens a property/value string, `:` moves to a value position
          trigger_characters: Some(vec!["\"".to_string(), ":".to_string()]),
          ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
      },
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
    let Some((file_path, notebook_path)) = self.resolve_format_paths(&params.text_document.uri) else {
      return Ok(None);
    };
    let Some((file_text, maybe_line_index)) = self.state.lock().documents.get_content(&params.text_document.uri) else {
      return Ok(None);
    };
    self
      .send_format_request(
        &params.text_document.uri,
        EditorFormatRequest {
          file_path,
          notebook_path,
          file_text,
          range: None,
          maybe_line_index,
          token: Arc::new(token),
        },
      )
      .await
  }

  async fn range_formatting(&self, params: DocumentRangeFormattingParams, token: CancellationToken) -> LspResult<Option<Vec<TextEdit>>> {
    let Some((file_path, notebook_path)) = self.resolve_format_paths(&params.text_document.uri) else {
      return Ok(None);
    };
    let Some((file_text, range, line_index)) = self.state.lock().documents.get_content_with_range(&params.text_document.uri, params.range) else {
      return Ok(None);
    };
    self
      .send_format_request(
        &params.text_document.uri,
        EditorFormatRequest {
          file_path,
          notebook_path,
          file_text,
          range,
          maybe_line_index: Some(line_index),
          token: Arc::new(token),
        },
      )
      .await
  }

  async fn completion(&self, params: CompletionParams, _token: CancellationToken) -> LspResult<Option<CompletionResponse>> {
    let uri = params.text_document_position.text_document.uri;
    if !is_config_uri(&uri) {
      return Ok(None);
    }
    let Some(file_path) = uri_to_file_path(&uri) else {
      return Ok(None);
    };
    let Some((file_text, _)) = self.state.lock().documents.get_content(&uri) else {
      return Ok(None);
    };
    let (sender, receiver) = oneshot::channel();
    let request = ConfigEditorRequest {
      file_path,
      file_text,
      position: params.text_document_position.position,
    };
    if self.sender.send(ChannelMessage::Completion(request, sender)).is_err() {
      return Ok(None);
    }
    Ok(receiver.await.ok().flatten().map(CompletionResponse::Array))
  }

  async fn hover(&self, params: HoverParams, _token: CancellationToken) -> LspResult<Option<Hover>> {
    let uri = params.text_document_position_params.text_document.uri;
    if !is_config_uri(&uri) {
      return Ok(None);
    }
    let Some(file_path) = uri_to_file_path(&uri) else {
      return Ok(None);
    };
    let Some((file_text, _)) = self.state.lock().documents.get_content(&uri) else {
      return Ok(None);
    };
    let (sender, receiver) = oneshot::channel();
    let request = ConfigEditorRequest {
      file_path,
      file_text,
      position: params.text_document_position_params.position,
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

fn canonicalize_path(environment: &impl Environment, path: PathBuf) -> PathBuf {
  environment.canonicalize_maybe_not_exists(&path).map(|p| p.into_path_buf()).unwrap_or(path)
}

#[cfg(test)]
mod test {
  use std::str::FromStr;
  use std::time::Duration;

  use deno_tower_lsp::lsp_types::ClientCapabilities;
  use deno_tower_lsp::lsp_types::DynamicRegistrationClientCapabilities;
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
  use deno_tower_lsp::lsp_types::TextDocumentClientCapabilities;
  use deno_tower_lsp::lsp_types::TextDocumentContentChangeEvent;
  use deno_tower_lsp::lsp_types::TextDocumentIdentifier;
  use deno_tower_lsp::lsp_types::TextDocumentItem;
  use deno_tower_lsp::lsp_types::VersionedNotebookDocumentIdentifier;
  use deno_tower_lsp::lsp_types::VersionedTextDocumentIdentifier;
  use dprint_core::async_runtime::future;

  use crate::environment::TestConfigFileBuilder;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::plugins::PluginCache;

  use super::client::ClientTrait;
  use super::*;

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

          // range formatting before the end of the cell keeps the trailing whitespace
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
            Some(vec![
              TextEdit {
                range: Range::new(Position::new(0, 1), Position::new(0, 1)),
                new_text: "_formatt".to_string()
              },
              TextEdit {
                range: Range::new(Position::new(0, 2), Position::new(0, 2)),
                new_text: "d\n_".to_string()
              },
              TextEdit {
                range: Range::new(Position::new(0, 6), Position::new(0, 6)),
                new_text: "_formatted  ".to_string()
              },
            ])
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

  fn setup_backend(environment: TestEnvironment) -> (Backend<TestEnvironment>, JoinHandle<()>, Arc<TestClient>) {
    setup_backend_with_config(environment, None)
  }

  fn setup_backend_with_config(environment: TestEnvironment, config_override: Option<PathBuf>) -> (Backend<TestEnvironment>, JoinHandle<()>, Arc<TestClient>) {
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache));
    let (tx, rx) = mpsc::unbounded_channel();
    let recv_task = start_message_handler(&environment, &plugin_resolver, config_override, rx);
    let test_client = Arc::new(TestClient::default());
    (Backend::new(ClientWrapper::new(test_client.clone()), environment, tx), recv_task, test_client)
  }

  #[derive(Debug, Default)]
  struct TestClient {
    logged_messages: Mutex<Vec<(MessageType, String)>>,
    registrations: Mutex<Vec<Registration>>,
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
  }

  impl ClientTrait for TestClient {
    fn log(&self, message_type: MessageType, message: String) {
      self.logged_messages.lock().push((message_type, message));
    }

    fn register_capabilities(&self, registrations: Vec<Registration>) {
      self.registrations.lock().extend(registrations);
    }
  }
}
