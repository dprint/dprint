use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::Client;
use deno_tower_lsp::lsp_types::Diagnostic;
use deno_tower_lsp::lsp_types::MessageActionItem;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::NumberOrString;
use deno_tower_lsp::lsp_types::ProgressParams;
use deno_tower_lsp::lsp_types::ProgressParamsValue;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::Uri;
use deno_tower_lsp::lsp_types::WorkDoneProgress;
use deno_tower_lsp::lsp_types::WorkDoneProgressCreateParams;
use deno_tower_lsp::lsp_types::WorkspaceEdit;
use deno_tower_lsp::lsp_types::notification::Notification;
use deno_tower_lsp::lsp_types::notification::Progress;
use deno_tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;

use super::workspace::messages::CONFIG_CHANGED_NOTIFICATION;
use super::workspace::messages::ConfigChangedParams;

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
  fn publish_diagnostics(&self, uri: Uri, diagnostics: Vec<Diagnostic>);
  /// Sends the `dprint/configChanged` notification.
  fn send_config_changed(&self, uri: Uri);
  /// Logs the message, which resolves once it's sent so that the messages
  /// are sent in order.
  fn send_log(&self, message_type: MessageType, message: String) -> LocalBoxFuture<'static, ()>;
  /// Asks the client to create the progress, which resolves to whether it did.
  fn create_work_done_progress(&self, token: NumberOrString) -> LocalBoxFuture<'static, bool>;
  fn send_progress(&self, token: NumberOrString, progress: WorkDoneProgress) -> LocalBoxFuture<'static, ()>;
}

/// The `dprint/configChanged` notification.
enum ConfigChangedNotification {}

impl Notification for ConfigChangedNotification {
  type Params = ConfigChangedParams;
  const METHOD: &'static str = CONFIG_CHANGED_NOTIFICATION;
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

  fn publish_diagnostics(&self, uri: Uri, diagnostics: Vec<Diagnostic>) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      client.publish_diagnostics(uri, diagnostics, None).await;
    });
  }

  fn send_config_changed(&self, uri: Uri) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      client.send_notification::<ConfigChangedNotification>(ConfigChangedParams { uri }).await;
    });
  }

  fn send_log(&self, message_type: MessageType, message: String) -> LocalBoxFuture<'static, ()> {
    let client = self.clone();
    async move { client.log_message(message_type, message).await }.boxed_local()
  }

  fn create_work_done_progress(&self, token: NumberOrString) -> LocalBoxFuture<'static, bool> {
    let client = self.clone();
    async move {
      client
        .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams { token })
        .await
        .is_ok()
    }
    .boxed_local()
  }

  fn send_progress(&self, token: NumberOrString, progress: WorkDoneProgress) -> LocalBoxFuture<'static, ()> {
    let client = self.clone();
    async move {
      client
        .send_notification::<Progress>(ProgressParams {
          token,
          value: ProgressParamsValue::WorkDone(progress),
        })
        .await
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

  pub fn publish_diagnostics(&self, uri: Uri, diagnostics: Vec<Diagnostic>) {
    self.0.publish_diagnostics(uri, diagnostics)
  }

  pub fn send_config_changed(&self, uri: Uri) {
    self.0.send_config_changed(uri)
  }

  pub fn send_log(&self, message_type: MessageType, message: String) -> LocalBoxFuture<'static, ()> {
    self.0.send_log(message_type, message)
  }

  pub fn create_work_done_progress(&self, token: NumberOrString) -> LocalBoxFuture<'static, bool> {
    self.0.create_work_done_progress(token)
  }

  pub fn send_progress(&self, token: NumberOrString, progress: WorkDoneProgress) -> LocalBoxFuture<'static, ()> {
    self.0.send_progress(token, progress)
  }

  fn log(&self, message_type: MessageType, message: String) {
    self.0.log(message_type, message)
  }
}
