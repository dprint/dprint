use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Semaphore;
use tokio::try_join;
use tokio_util::sync::CancellationToken;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::DidChangeTextDocumentParams;
use tower_lsp::lsp_types::DidCloseTextDocumentParams;
use tower_lsp::lsp_types::DidOpenTextDocumentParams;
use tower_lsp::lsp_types::DocumentFormattingParams;
use tower_lsp::lsp_types::DocumentRangeFormattingParams;
use tower_lsp::lsp_types::InitializeParams;
use tower_lsp::lsp_types::InitializeResult;
use tower_lsp::lsp_types::InitializedParams;
use tower_lsp::lsp_types::OneOf;
use tower_lsp::lsp_types::ServerCapabilities;
use tower_lsp::lsp_types::ServerInfo;
use tower_lsp::lsp_types::TextDocumentSyncCapability;
use tower_lsp::lsp_types::TextDocumentSyncKind;
use tower_lsp::lsp_types::TextDocumentSyncOptions;
use tower_lsp::lsp_types::TextEdit;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::environment::Environment;
use crate::plugins::PluginResolver;

use self::client::ClientWrapper;
use self::config::LspPluginsScopeContainer;
use self::documents::Documents;
use self::text::get_edits;
use self::text::LineIndex;

mod client;
mod config;
mod documents;
mod text;

// tower-lsp will drop the future on cancellation,
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
    for (_, token) in pending_tokens.iter() {
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
  pub file_path: PathBuf,
  pub file_text: String,
  pub maybe_line_index: Option<LineIndex>,
  pub range: FormatRange,
  pub token: Arc<CancellationToken>,
}

enum ChannelMessage {
  Format(EditorFormatRequest, oneshot::Sender<Result<Option<Vec<TextEdit>>>>),
  Shutdown(oneshot::Sender<()>),
}

async fn handle_format_request<TEnvironment: Environment>(
  mut request: EditorFormatRequest,
  scope_container: Rc<LspPluginsScopeContainer<TEnvironment>>,
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
  // canonicalize the path
  request.file_path = environment
    .canonicalize(&request.file_path)
    .map(|p| p.into_path_buf())
    .unwrap_or(request.file_path);

  if !scope.can_format_for_editor(&request.file_path) {
    log_debug!(environment, "Excluded file: {}", request.file_path.display());
    return Ok(None);
  }

  let Some(result) = scope
    .format(HostFormatRequest {
      file_path: request.file_path,
      file_bytes: request.file_text.as_bytes().to_vec(),
      range: request.range,
      override_config: Default::default(),
      token: request.token,
    })
    .await?
  else {
    return Ok(None);
  };
  dprint_core::async_runtime::spawn_blocking(move || {
    let new_text = String::from_utf8(result).context("Failed converting formatted text to utf-8.")?;
    let line_index = request.maybe_line_index.unwrap_or_else(|| LineIndex::new(&request.file_text));
    Ok(Some(get_edits(&request.file_text, &new_text, &line_index)))
  })
  .await?
}

pub async fn run_language_server<TEnvironment: Environment>(
  _args: &CliArgs,
  environment: &TEnvironment,
  _plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> anyhow::Result<()> {
  let stdin = tokio::io::stdin();
  let stdout = tokio::io::stdout();
  let (tx, mut rx) = mpsc::unbounded_channel();

  // tower_lsp requires Backend to implement Send and Sync, but
  // we use a single threaded runtime. So spawn some tasks and
  // communicate over a channel.
  let recv_task = {
    let max_cores = environment.max_threads();
    let concurrency_limiter = Rc::new(Semaphore::new(std::cmp::max(1, max_cores - 1)));
    let environment = environment.clone();
    let scope_container = Rc::new(LspPluginsScopeContainer::new(environment.clone()));
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
              let result = handle_format_request(request, scope_container, &environment).await;
              let _ = sender.send(result);
              drop(token_guard); // remove the token from the pending tokens
            });
          }
          ChannelMessage::Shutdown(sender) => {
            pending_tokens.cancel_all();
            scope_container.shutdown().await;
            let _ = sender.send(());
            break; // exit
          }
        }
      }
    })
  };

  let environment = environment.clone();
  let lsp_task = dprint_core::async_runtime::spawn(async move {
    let (service, socket) = LspService::new(|client| {
      let client = ClientWrapper::new(client);
      Backend {
        client: client.clone(),
        environment: environment.clone(),
        sender: tx,
        state: Mutex::new(State {
          documents: Documents::new(environment),
        }),
      }
    });
    Server::new(stdin, stdout, socket).serve(service).await;
  });

  try_join!(recv_task, lsp_task)?;

  Ok(())
}

struct State<TEnvironment: Environment> {
  documents: Documents<TEnvironment>,
}

struct Backend<TEnvironment: Environment> {
  client: ClientWrapper,
  environment: TEnvironment,
  sender: mpsc::UnboundedSender<ChannelMessage>,
  state: Mutex<State<TEnvironment>>,
}

impl<TEnvironment: Environment> Backend<TEnvironment> {
  async fn send_format_request(&self, request: EditorFormatRequest) -> Result<Option<Vec<TextEdit>>> {
    let mut drop_token = DropToken::new(request.token.clone());
    let result = self.send_format_request_inner(request).await;
    drop_token.completed();
    result
  }

  async fn send_format_request_inner(&self, request: EditorFormatRequest) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    self.sender.send(ChannelMessage::Format(request, sender))?;
    receiver.await?
  }
}

#[tower_lsp::async_trait]
impl<TEnvironment: Environment> LanguageServer for Backend<TEnvironment> {
  async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
    if let Some(parent_id) = params.process_id {
      start_parent_process_checker_task(parent_id);
    }

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
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
      },
    })
  }

  async fn initialized(&self, _: InitializedParams) {
    self.client.log_info(format!(
      "dprint {} ({}_{})",
      self.environment.cli_version(),
      self.environment.os(),
      self.environment.cpu_arch()
    ));
    self.client.log_info("Server ready.".to_string());
  }

  async fn did_open(&self, params: DidOpenTextDocumentParams) {
    self.state.lock().documents.open(params.text_document);
  }

  async fn did_change(&self, params: DidChangeTextDocumentParams) {
    self.state.lock().documents.changed(params);
  }

  async fn did_close(&self, params: DidCloseTextDocumentParams) {
    self.state.lock().documents.closed(params);
  }

  async fn formatting(&self, params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
    let Some(file_path) = url_to_file_path(&params.text_document.uri) else {
      return Ok(None);
    };
    let Some((file_text, maybe_line_index)) = self.state.lock().documents.get_content(&params.text_document.uri) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let result = self
      .send_format_request(EditorFormatRequest {
        file_path,
        file_text,
        range: None,
        maybe_line_index,
        token: token.clone(),
      })
      .await;
    match result {
      Ok(value) => Ok(value),
      Err(err) => {
        log_error!(self.environment, "Failed formatting '{}': {:#}", params.text_document.uri, err);
        Ok(None)
      }
    }
  }

  async fn range_formatting(&self, params: DocumentRangeFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
    let Some(file_path) = url_to_file_path(&params.text_document.uri) else {
      return Ok(None);
    };
    let Some((file_text, range, line_index)) = self.state.lock().documents.get_content_with_range(&params.text_document.uri, params.range) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let result = self
      .send_format_request(EditorFormatRequest {
        file_path,
        file_text,
        range,
        maybe_line_index: Some(line_index),
        token: token.clone(),
      })
      .await;
    match result {
      Ok(value) => Ok(value),
      Err(err) => {
        log_error!(self.environment, "Failed formatting '{}': {:#}", params.text_document.uri, err);
        Ok(None)
      }
    }
  }

  async fn shutdown(&self) -> LspResult<()> {
    let (sender, receiver) = oneshot::channel();
    if self.sender.send(ChannelMessage::Shutdown(sender)).is_ok() {
      let _ = receiver.await;
    };
    Ok(())
  }
}

fn url_to_file_path(url: &Url) -> Option<PathBuf> {
  if url.scheme() == "file" {
    url.to_file_path().ok()
  } else {
    None
  }
}
