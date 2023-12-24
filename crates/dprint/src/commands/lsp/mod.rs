use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use dprint_core::plugins::process::start_parent_process_checker_task;
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

enum ChannelMessage {
  Format(HostFormatRequest, oneshot::Sender<Result<Option<Vec<TextEdit>>>>),
  Shutdown(oneshot::Sender<()>),
}

async fn handle_format_request<TEnvironment: Environment>(
  mut request: HostFormatRequest,
  scope_container: Rc<LspPluginsScopeContainer<TEnvironment>>,
  environment: &TEnvironment,
) -> Result<Option<Vec<TextEdit>>> {
  let Some(parent_dir) = request.file_path.parent() else {
    log_warn!(environment, "Cannot format non-file path: {}", request.file_path.display());
    return Ok(None);
  };
  let Some(scope) = scope_container.resolve_by_path(parent_dir).await? else {
    log_stderr_info!(environment, "Path did not have a dprint config file: {}", request.file_path.display());
    return Ok(None);
  };
  // canonicalize the path
  request.file_path = environment
    .canonicalize(&request.file_path)
    .map(|p| p.into_path_buf())
    .unwrap_or(request.file_path);

  if !scope.can_format_for_editor(&request.file_path) {
    log_debug!(environment, "Excluded file: {}", request.file_path.display());
    return Ok(None);
  }

  let original_text = request.file_bytes.clone();
  let Some(result) = scope.format(request).await? else {
    return Ok(None);
  };
  dprint_core::async_runtime::spawn_blocking(|| {
    // todo: don't do this conversion from original bytes to string
    let original_text = String::from_utf8(original_text)?;
    let new_text = String::from_utf8(result)?;
    // todo: pass the line index into here as well so it doesn't need to be recomputed
    Ok(Some(get_edits(&original_text, &new_text, &LineIndex::new(&original_text))))
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
      while let Some(message) = rx.recv().await {
        match message {
          ChannelMessage::Format(request, sender) => {
            let concurrency_limiter = concurrency_limiter.clone();
            let scope_container = scope_container.clone();
            let environment = environment.clone();
            dprint_core::async_runtime::spawn(async move {
              let _permit = concurrency_limiter.acquire().await;
              if request.token.is_cancelled() {
                return;
              }
              let result = handle_format_request(request, scope_container, &environment).await;
              let _ = sender.send(result);
            });
          }
          ChannelMessage::Shutdown(sender) => {
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
          documents: Documents::new(client, environment),
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
  async fn send_format_request(&self, request: HostFormatRequest, token: Arc<CancellationToken>) -> Result<Option<Vec<TextEdit>>> {
    let mut drop_token = DropToken::new(token.clone());
    let result = self.send_format_request_inner(request).await;
    drop_token.completed();
    result
  }

  async fn send_format_request_inner(&self, request: HostFormatRequest) -> Result<Option<Vec<TextEdit>>> {
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
    let Some(file_text) = self.state.lock().documents.get_content(&params.text_document.uri) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let result = self
      .send_format_request(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.into_bytes(),
          range: None,
          override_config: Default::default(),
          token: token.clone(),
        },
        token,
      )
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
    let Some((file_text, range)) = self.state.lock().documents.get_content_with_range(&params.text_document.uri, params.range) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let result = self
      .send_format_request(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.into_bytes(),
          range,
          override_config: Default::default(),
          token: token.clone(),
        },
        token,
      )
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
