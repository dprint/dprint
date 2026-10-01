use std::sync::Arc;

use deno_tower_lsp::Client;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::Registration;

pub trait ClientTrait: std::fmt::Debug + Send + Sync {
  fn log(&self, message_type: MessageType, message: String);
  fn register_capabilities(&self, registrations: Vec<Registration>);
}

impl ClientTrait for Client {
  fn log(&self, message_type: MessageType, message: String) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      client.log_message(message_type, &message).await;
    });
  }

  fn register_capabilities(&self, registrations: Vec<Registration>) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      if let Err(err) = client.register_capability(registrations).await {
        client
          .log_message(MessageType::WARNING, format!("Failed registering capabilities: {:#}", err))
          .await;
      }
    });
  }
}

#[derive(Debug, Clone)]
pub struct ClientWrapper(Arc<dyn ClientTrait>);

impl ClientWrapper {
  pub fn new(client: Arc<dyn ClientTrait>) -> Self {
    Self(client)
  }

  pub fn log_info(&self, message: String) {
    self.log(MessageType::INFO, message);
  }

  pub fn log_warn(&self, message: String) {
    self.log(MessageType::WARNING, message);
  }

  pub fn log_error(&self, message: String) {
    self.log(MessageType::ERROR, message);
  }

  pub fn register_capabilities(&self, registrations: Vec<Registration>) {
    self.0.register_capabilities(registrations)
  }

  fn log(&self, message_type: MessageType, message: String) {
    self.0.log(message_type, message)
  }
}
