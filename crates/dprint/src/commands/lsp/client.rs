use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::Client;
use deno_tower_lsp::lsp_types::MessageActionItem;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::WorkspaceEdit;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;

pub trait ClientTrait: std::fmt::Debug + Send + Sync {
  fn log(&self, message_type: MessageType, message: String);
  fn register_capabilities(&self, registrations: Vec<Registration>);
  fn show_message(&self, message_type: MessageType, message: String);
  /// Shows a message with actions, resolving to the one the user selected.
  fn show_message_request(
    &self,
    message_type: MessageType,
    message: String,
    actions: Vec<MessageActionItem>,
  ) -> LocalBoxFuture<'static, Option<MessageActionItem>>;
  fn apply_edit(&self, edit: WorkspaceEdit) -> LocalBoxFuture<'static, Result<()>>;
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

  fn show_message(&self, message_type: MessageType, message: String) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      client.show_message(message_type, &message).await;
    });
  }

  fn show_message_request(
    &self,
    message_type: MessageType,
    message: String,
    actions: Vec<MessageActionItem>,
  ) -> LocalBoxFuture<'static, Option<MessageActionItem>> {
    let client = self.clone();
    async move {
      match client.show_message_request(message_type, &message, Some(actions)).await {
        Ok(selection) => selection,
        Err(err) => {
          client.log_message(MessageType::WARNING, format!("Failed showing message: {:#}", err)).await;
          None
        }
      }
    }
    .boxed_local()
  }

  fn apply_edit(&self, edit: WorkspaceEdit) -> LocalBoxFuture<'static, Result<()>> {
    let client = self.clone();
    async move {
      let response = client.apply_edit(edit).await?;
      if !response.applied {
        bail!("{}", response.failure_reason.as_deref().unwrap_or("The client did not apply the edit."));
      }
      Ok(())
    }
    .boxed_local()
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

  pub fn show_message(&self, message_type: MessageType, message: String) {
    self.0.show_message(message_type, message)
  }

  pub fn show_message_request(
    &self,
    message_type: MessageType,
    message: String,
    actions: Vec<MessageActionItem>,
  ) -> LocalBoxFuture<'static, Option<MessageActionItem>> {
    self.0.show_message_request(message_type, message, actions)
  }

  pub fn apply_edit(&self, edit: WorkspaceEdit) -> LocalBoxFuture<'static, Result<()>> {
    self.0.apply_edit(edit)
  }

  fn log(&self, message_type: MessageType, message: String) {
    self.0.log(message_type, message)
  }
}
