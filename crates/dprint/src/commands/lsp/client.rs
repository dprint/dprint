use std::sync::Arc;

use deno_tower_lsp::Client;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::Unregistration;

pub trait ClientTrait: std::fmt::Debug + Send + Sync {
  fn log(&self, message_type: MessageType, message: String);
  fn register_capabilities(&self, registrations: Vec<Registration>);
  fn unregister_capabilities(&self, unregistrations: Vec<Unregistration>);
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

  fn unregister_capabilities(&self, unregistrations: Vec<Unregistration>) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      if let Err(err) = client.unregister_capability(unregistrations).await {
        client
          .log_message(MessageType::WARNING, format!("Failed unregistering capabilities: {:#}", err))
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

  pub fn register_capabilities(&self, registrations: Vec<Registration>) {
    self.0.register_capabilities(registrations)
  }

  pub fn unregister_capabilities(&self, unregistrations: Vec<Unregistration>) {
    self.0.unregister_capabilities(unregistrations)
  }

  fn log(&self, message_type: MessageType, message: String) {
    self.0.log(message_type, message)
  }
}
