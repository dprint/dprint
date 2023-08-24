use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
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
use tower_lsp::lsp_types::TextDocumentSyncCapability;
use tower_lsp::lsp_types::TextDocumentSyncKind;
use tower_lsp::lsp_types::TextDocumentSyncOptions;
use tower_lsp::lsp_types::TextEdit;
use tower_lsp::Client;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;

use crate::arg_parser::CliArgs;
use crate::environment::Environment;
use crate::plugins::PluginResolver;

#[derive(Debug)]
enum ChannelMessage {
  Format(PathBuf, FormatRange, String, oneshot::Sender<Result<Option<Vec<TextEdit>>>>),
}

struct Backend {
  client: Client,
  sender: mpsc::UnboundedSender<ChannelMessage>,
}

pub async fn run_language_server<TEnvironment: Environment>(
  _args: &CliArgs,
  _environment: &TEnvironment,
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
        ChannelMessage::Format(file_path, range, file_text, sender) => {
          dprint_core::async_runtime::spawn(async move {
            // TODO: Send the actual format request.
            let token = Arc::new(CancellationToken::new());
            let _request = HostFormatRequest {
              file_path,
              file_text,
              range,
              override_config: Default::default(), // TODO: Set from params?
              token,
            };
            // TODO: How to get the plugin scope?
            // TODO: Return back an actual response
            let _ = sender.send(Ok(None));
          });
        }
      }
    }
  });

  let lsp_task = dprint_core::async_runtime::spawn(async move {
    let (service, socket) = LspService::new(|client| Backend { client, sender: tx });
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

    Ok(InitializeResult {
      // TODO: Any server info we want to include here?
      server_info: None,
      capabilities: ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
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
    self.client.log_message(MessageType::INFO, "Server initialized.").await;
  }

  async fn did_open(&self, _: DidOpenTextDocumentParams) {
    todo!()
  }

  async fn did_change(&self, _: DidChangeTextDocumentParams) {
    todo!()
  }

  async fn did_close(&self, _: DidCloseTextDocumentParams) {
    todo!()
  }

  async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    let file_path = params.text_document.uri.to_file_path().expect("Expected a valid path");
    let range = Default::default(); // TODO: Set it based on the file length
    let file_text = "".into(); // TODO: Fetch from the latest document sync message
    self.sender.send(ChannelMessage::Format(file_path, range, file_text, sender)).unwrap();
    receiver.await.unwrap()
  }

  async fn range_formatting(&self, params: DocumentRangeFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    let (sender, receiver) = oneshot::channel();
    let file_path = params.text_document.uri.to_file_path().expect("Expected a valid path");
    let range = Default::default(); // TODO: Map LSP range to FormatRange
    let file_text = "".into(); // TODO: Fetch from the latest document sync message
    self.sender.send(ChannelMessage::Format(file_path, range, file_text, sender)).unwrap();
    receiver.await.unwrap()
  }

  async fn shutdown(&self) -> Result<()> {
    todo!()
  }
}
