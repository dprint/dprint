use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;

#[derive(Debug, Clone)]
pub struct ClientWrapper(Client);

impl ClientWrapper {
  pub fn new(client: Client) -> Self {
    Self(client)
  }

  pub fn log_info(&self, message: String) {
    self.log(MessageType::INFO, message);
  }

  pub fn log_warning(&self, message: String) {
    self.log(MessageType::WARNING, message);
  }

  pub fn log_error(&self, message: String) {
    self.log(MessageType::ERROR, message);
  }

  fn log(&self, message_type: MessageType, message: String) {
    let client = self.0.clone();
    dprint_core::async_runtime::spawn(async move {
      client.log_message(message_type, &message).await;
    });
  }
}
