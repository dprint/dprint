use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::try_join;
use tokio_util::sync::CancellationToken;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::DidChangeTextDocumentParams;
use tower_lsp::lsp_types::DidCloseTextDocumentParams;
use tower_lsp::lsp_types::DidOpenTextDocumentParams;
use tower_lsp::lsp_types::DocumentFormattingParams;
use tower_lsp::lsp_types::DocumentRangeFormattingParams;
use tower_lsp::lsp_types::InitializeParams;
use tower_lsp::lsp_types::InitializeResult;
use tower_lsp::lsp_types::InitializedParams;
use tower_lsp::lsp_types::MessageType;
use tower_lsp::lsp_types::OneOf;
use tower_lsp::lsp_types::ServerCapabilities;
use tower_lsp::lsp_types::ServerInfo;
use tower_lsp::lsp_types::TextDocumentSyncCapability;
use tower_lsp::lsp_types::TextDocumentSyncKind;
use tower_lsp::lsp_types::TextDocumentSyncOptions;
use tower_lsp::lsp_types::TextEdit;
use tower_lsp::Client;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::environment::Environment;
use crate::plugins::PluginResolver;

use self::documents::Documents;

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
}

struct State {
  documents: Documents,
}

struct Backend {
  client: Client,
  sender: mpsc::UnboundedSender<ChannelMessage>,
  state: Mutex<State>,
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
  let recv_task = dprint_core::async_runtime::spawn(async move {
    while let Some(message) = rx.recv().await {
      match message {
        ChannelMessage::Format(request, sender) => {
          dprint_core::async_runtime::spawn(async move {
            // TODO: Send the actual format request.
            // TODO: How to get the plugin scope?
            // TODO: Return back an actual response
            let _ = sender.send(Ok(None));
          });
        }
      }
    }
  });

  let lsp_task = dprint_core::async_runtime::spawn(async move {
    let (service, socket) = LspService::new(|client| Backend {
      client: client.clone(),
      sender: tx,
      state: Mutex::new(State {
        documents: Documents::new(client),
      }),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
  });

  try_join!(recv_task, lsp_task)?;

  Ok(())
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
  async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
    if let Some(parent_id) = params.process_id {
      start_parent_process_checker_task(parent_id);
    }

    // todo: use root_uri or workspace_folder to determine where to search
    // for dprint.json files and then watch those paths for changes that create
    // a dprint.json file

    Ok(InitializeResult {
      server_info: Some(ServerInfo {
        name: "dprint".to_string(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
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
    // todo: log more information probably
    self.client.log_message(MessageType::INFO, "Server initialized.").await;
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

  async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    let Some(file_path) = url_to_file_path(&params.text_document.uri) else {
      return Ok(None);
    };
    let Some(file_text) = self.state.lock().documents.get_content(&params.text_document.uri) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let mut drop_token = DropToken::new(token.clone());
    self
      .sender
      .send(ChannelMessage::Format(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.into_bytes(),
          range: None,
          override_config: Default::default(),
          token,
        },
        sender,
      ))
      .unwrap();
    let result = receiver.await.unwrap();
    drop_token.completed();
    result
  }

  async fn range_formatting(&self, params: DocumentRangeFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    let Some(file_path) = url_to_file_path(&params.text_document.uri) else {
      return Ok(None);
    };
    let range = Default::default(); // TODO: Map LSP range to FormatRange
    let Some(file_text) = self.state.lock().documents.get_content(&params.text_document.uri) else {
      return Ok(None);
    };
    let token = Arc::new(CancellationToken::new());
    let mut drop_token = DropToken::new(token.clone());
    self
      .sender
      .send(ChannelMessage::Format(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.into_bytes(),
          range,
          override_config: Default::default(),
          token,
        },
        sender,
      ))
      .unwrap();
    let result = receiver.await.unwrap();
    drop_token.completed();
    result
  }

  async fn shutdown(&self) -> Result<()> {
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
