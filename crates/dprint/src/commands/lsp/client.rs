use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::Client;
use deno_tower_lsp::lsp_types::Diagnostic;
use deno_tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions;
use deno_tower_lsp::lsp_types::FileSystemWatcher;
use deno_tower_lsp::lsp_types::GlobPattern;
use deno_tower_lsp::lsp_types::MessageActionItem;
use deno_tower_lsp::lsp_types::MessageType;
use deno_tower_lsp::lsp_types::NumberOrString;
use deno_tower_lsp::lsp_types::OneOf;
use deno_tower_lsp::lsp_types::ProgressParams;
use deno_tower_lsp::lsp_types::ProgressParamsValue;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::RelativePattern;
use deno_tower_lsp::lsp_types::Unregistration;
use deno_tower_lsp::lsp_types::Uri;
use deno_tower_lsp::lsp_types::WorkDoneProgress;
use deno_tower_lsp::lsp_types::WorkDoneProgressCreateParams;
use deno_tower_lsp::lsp_types::WorkspaceEdit;
use deno_tower_lsp::lsp_types::notification::Notification;
use deno_tower_lsp::lsp_types::notification::Progress;
use deno_tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;

use super::file_path_to_uri;
use super::workspace::messages::CONFIG_CHANGED_NOTIFICATION;
use super::workspace::messages::ConfigChangedParams;

/// The id of the registration of the files outside the workspace that the
/// client is asked to watch (see `ClientWrapper::register_watched_files`).
const WATCHED_FILES_REGISTRATION_ID: &str = "dprint-watched-files";
const DID_CHANGE_WATCHED_FILES_METHOD: &str = "workspace/didChangeWatchedFiles";

pub trait ClientTrait: std::fmt::Debug + Send + Sync {
  fn log(&self, message_type: MessageType, message: String);
  fn register_capabilities(&self, registrations: Vec<Registration>);
  /// Unregisters the capability with the id of the registration, which is
  /// then registered again when it's provided.
  fn replace_capability(&self, unregistration: Unregistration, registration: Option<Registration>);
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
  fn send_config_changed(&self, params: ConfigChangedParams);
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

  fn replace_capability(&self, unregistration: Unregistration, registration: Option<Registration>) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      // it fails when it wasn't registered before, which is fine
      let _ = client.unregister_capability(vec![unregistration]).await;
      if let Some(registration) = registration
        && let Err(err) = client.register_capability(vec![registration]).await
      {
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

  fn send_config_changed(&self, params: ConfigChangedParams) {
    let client = self.clone();
    dprint_core::async_runtime::spawn(async move {
      client.send_notification::<ConfigChangedNotification>(params).await;
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

  /// Asks the client to tell the server when the files change with
  /// `workspace/didChangeWatchedFiles`, which replaces the files it was asked
  /// to watch before.
  pub fn register_watched_files(&self, file_paths: &[PathBuf]) {
    let watchers = file_paths
      .iter()
      .filter_map(|file_path| {
        // each file is watched in its directory because a glob is relative to the workspace
        let dir_uri = file_path_to_uri(file_path.parent()?)?;
        let file_name = file_path.file_name()?.to_str()?;
        Some(FileSystemWatcher {
          glob_pattern: GlobPattern::Relative(RelativePattern {
            base_uri: OneOf::Right(dir_uri),
            pattern: escape_glob(file_name),
          }),
          kind: None,
        })
      })
      .collect::<Vec<_>>();
    let registration = (!watchers.is_empty()).then(|| Registration {
      id: WATCHED_FILES_REGISTRATION_ID.to_string(),
      method: DID_CHANGE_WATCHED_FILES_METHOD.to_string(),
      register_options: Some(serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers }).expect("options should serialize")),
    });
    let unregistration = Unregistration {
      id: WATCHED_FILES_REGISTRATION_ID.to_string(),
      method: DID_CHANGE_WATCHED_FILES_METHOD.to_string(),
    };
    self.0.replace_capability(unregistration, registration)
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

  pub fn send_config_changed(&self, params: ConfigChangedParams) {
    self.0.send_config_changed(params)
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

/// Escapes the characters of the text that a glob treats specially.
fn escape_glob(text: &str) -> String {
  let mut glob = String::with_capacity(text.len());
  for c in text.chars() {
    if matches!(c, '*' | '?' | '[' | '{' | '}') {
      glob.push('[');
      glob.push(c);
      glob.push(']');
    } else {
      glob.push(c);
    }
  }
  glob
}
