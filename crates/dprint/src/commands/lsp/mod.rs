use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::DidChangeTextDocumentParams;
use tower_lsp::lsp_types::DidCloseTextDocumentParams;
use tower_lsp::lsp_types::DidOpenTextDocumentParams;
use tower_lsp::lsp_types::DocumentFormattingParams;
use tower_lsp::lsp_types::DocumentRangeFormattingParams;
use tower_lsp::lsp_types::InitializeParams;
use tower_lsp::lsp_types::InitializeResult;
use tower_lsp::lsp_types::OneOf;
use tower_lsp::lsp_types::ServerCapabilities;
use tower_lsp::lsp_types::TextDocumentSyncCapability;
use tower_lsp::lsp_types::TextDocumentSyncKind;
use tower_lsp::lsp_types::TextDocumentSyncOptions;
use tower_lsp::lsp_types::TextEdit;
use tower_lsp::LanguageServer;

struct Backend;

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
  async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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

  async fn did_open(&self, params: DidOpenTextDocumentParams) {
    todo!()
  }

  async fn did_change(&self, _: DidChangeTextDocumentParams) {
    todo!()
  }

  async fn did_close(&self, _: DidCloseTextDocumentParams) {
    todo!()
  }

  async fn formatting(&self, _: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    todo!()
  }

  async fn range_formatting(&self, _: DocumentRangeFormattingParams) -> Result<Option<Vec<TextEdit>>> {
    todo!()
  }

  async fn shutdown(&self) -> Result<()> {
    todo!()
  }
}
