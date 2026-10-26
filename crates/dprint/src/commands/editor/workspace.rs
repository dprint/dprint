//! The editor service of schema version 6, which formats the files of a whole
//! workspace in one process by finding the config file of each file, the same
//! as the language server.

use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use dprint_core::communication::RcIdStore;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::arg_parser::CliArgs;
use crate::commands::lsp::notebook::trim_formatted_cell_text;
use crate::commands::lsp::text::get_utf16_edits;
use crate::commands::lsp::text::normalize_to_source_line_endings;
use crate::commands::no_config::DISMISS_ACTION_TITLE;
use crate::commands::no_config::DISMISS_WORKSPACE_ACTION_TITLE;
use crate::commands::no_config::EDITOR_EXTENSION_SETTINGS_OWNER;
use crate::commands::no_config::NoConfigMessageOptions;
use crate::commands::no_config::NoConfigNotificationDismissal;
use crate::commands::no_config::dismiss_no_config_notification;
use crate::commands::no_config::get_no_config_message;
use crate::commands::no_config::is_no_config_notification_dismissed;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::configuration::resolve_global_config_path_and_text;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::paths::get_plugin_names_for_file_on_disk;
use crate::plugins::PluginResolver;
use crate::resolution::PluginsScope;
use crate::utils::Semaphore;

use super::EditorContext;
use super::get_editor_plugin_infos;
use super::messages::ClientInfo;
use super::messages::ConfigFileInfo;
use super::messages::ConfigInfoMessage;
use super::messages::ConfigInfoResponseMessage;
use super::messages::EditorMessage;
use super::messages::EditorMessageBody;
use super::messages::EditorSettings;
use super::messages::FileInfoMessage;
use super::messages::FileInfoResponseMessage;
use super::messages::FormatEdit;
use super::messages::FormatFileMessageBody;
use super::messages::FormatFileOptions;
use super::messages::FormatFileResult;
use super::messages::FormattedMessage;
use super::messages::InitializeMessage;
use super::messages::MessageAction;
use super::messages::MessageKind;
use super::messages::NotFormattedMessage;
use super::messages::NotFormattedReason;
use super::messages::ShowMessageMessage;
use super::messages::ShowMessageRequestMessage;
use super::messages::ShowMessageRequestResponseMessage;
use super::new_format_concurrency_limiter;
use super::send_error_response;
use super::send_response_body;
use super::start_reading_messages;

struct WorkspaceEditorService<TEnvironment: Environment> {
  environment: TEnvironment,
  context: EditorContext,
  scope_container: PluginsScopeContainer<TEnvironment>,
  concurrency_limiter: Rc<Semaphore>,
  ensure_stable_format: EnsureStableFormat,
  /// The editor, which is `None` until it initializes the editor service.
  client_info: RefCell<Option<ClientInfo>>,
  settings: RefCell<EditorSettings>,
  /// The responses being waited on for the `ShowMessageRequest` messages.
  pending_message_requests: RcIdStore<oneshot::Sender<Option<MessageAction>>>,
  /// Whether the user was notified this session about a file without a config file.
  has_notified_no_config: Cell<bool>,
}

/// The outcome of formatting a file.
enum FormatOutcome {
  Formatted(Vec<FormatEdit>),
  NotFormatted(NotFormattedReason),
}

pub async fn run_workspace_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config_path = args.config.as_ref().map(|config| environment.cwd().join(config));
  let service = Rc::new(WorkspaceEditorService {
    environment: environment.clone(),
    context: EditorContext::new(environment),
    scope_container: PluginsScopeContainer::new(environment.clone(), plugin_resolver.clone(), config_path, args.remote_cache_mode()),
    concurrency_limiter: Rc::new(new_format_concurrency_limiter(environment)),
    ensure_stable_format: EnsureStableFormat::for_editor(environment),
    client_info: Default::default(),
    settings: Default::default(),
    pending_message_requests: Default::default(),
    has_notified_no_config: Default::default(),
  });
  let mut rx = start_reading_messages(environment, 6);

  loop {
    let message = tokio::select! {
      message = rx.recv() => message,
      // there's nobody to respond to anymore
      _ = service.context.output_closed.cancelled() => None,
    };
    let Some(message) = message else {
      return Ok(());
    };
    let context = &service.context;
    if service.client_info.borrow().is_none() && requires_initialize(&message.body) {
      send_error_response(
        context,
        message.id,
        anyhow!("Send an initialize message before message kind {}.", message.body.as_u32()),
      );
      continue;
    }
    match message.body {
      EditorMessageBody::Success(_) => {}
      EditorMessageBody::Error(message_id, _) => {
        // not choosing an action when the editor failed showing the message
        if let Some(sender) = service.pending_message_requests.take(message_id) {
          let _ = sender.send(None);
        }
      }
      EditorMessageBody::Close => {
        send_response_body(context, EditorMessageBody::Success(message.id));
        return Ok(());
      }
      EditorMessageBody::IsAlive => {
        send_response_body(context, EditorMessageBody::Success(message.id));
      }
      EditorMessageBody::Initialize(data) => {
        if service.client_info.borrow().is_some() {
          send_error_response(context, message.id, anyhow!("The editor service was already initialized."));
          continue;
        }
        match serde_json::from_slice::<InitializeMessage>(&data) {
          Ok(data) => {
            let client_info = data.client_info;
            log_debug!(
              environment,
              "Initialized the editor service for {} {}",
              client_info.name,
              client_info.version.as_deref().unwrap_or("(unknown version)")
            );
            *service.settings.borrow_mut() = data.settings;
            *service.client_info.borrow_mut() = Some(client_info);
            send_response_body(context, EditorMessageBody::Success(message.id));
          }
          Err(err) => send_error_response(context, message.id, anyhow!("Error deserializing the initialize message. {:#}", err)),
        }
      }
      EditorMessageBody::UpdateSettings(data) => match serde_json::from_slice(&data) {
        Ok(settings) => {
          *service.settings.borrow_mut() = settings;
          send_response_body(context, EditorMessageBody::Success(message.id));
        }
        Err(err) => send_error_response(context, message.id, anyhow!("Error deserializing the settings. {:#}", err)),
      },
      EditorMessageBody::ShowMessageRequestResponse(message_id, data) => {
        if let Some(sender) = service.pending_message_requests.take(message_id) {
          match serde_json::from_slice::<ShowMessageRequestResponseMessage>(&data) {
            Ok(response) => {
              let _ = sender.send(response.action);
            }
            // dropping the sender is the same as not choosing an action
            Err(err) => send_error_response(context, message.id, anyhow!("Error deserializing the show message request response. {:#}", err)),
          }
        }
      }
      EditorMessageBody::FileInfo(data) => {
        // these are handled at the same time, which the scope container
        // handles by only resolving each config file once
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = async {
            let message = serde_json::from_slice(&data).context("Error deserializing the file info message.")?;
            let response = service.get_file_info(message).await?;
            Ok::<_, anyhow::Error>(serde_json::to_vec(&response)?)
          }
          .await;
          match result {
            Ok(data) => send_response_body(&service.context, EditorMessageBody::FileInfoResponse(message.id, data)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::ConfigInfo(data) => {
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = async {
            let message = serde_json::from_slice(&data).context("Error deserializing the config info message.")?;
            let response = service.get_config_info(message).await?;
            Ok::<_, anyhow::Error>(serde_json::to_vec(&response)?)
          }
          .await;
          match result {
            Ok(data) => send_response_body(&service.context, EditorMessageBody::ConfigInfoResponse(message.id, data)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::FormatFile(body) => {
        // store this now so the format can be cancelled while it's waiting to be handled
        let token = Arc::new(CancellationToken::new());
        let token_storage_guard = context.cancellation_tokens.store_with_owned_guard(message.id, token.clone());
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = service.format_file(body, &token).await;
          drop(token_storage_guard);
          // the editor isn't waiting on a cancelled format
          if token.is_cancelled() {
            return;
          }
          let result = result.and_then(|outcome| {
            Ok(match outcome {
              FormatOutcome::Formatted(edits) => FormatFileResult::Formatted(serde_json::to_vec(&FormattedMessage { edits })?),
              FormatOutcome::NotFormatted(reason) => FormatFileResult::NotFormatted(serde_json::to_vec(&NotFormattedMessage { reason })?),
            })
          });
          match result {
            Ok(result) => send_response_body(&service.context, EditorMessageBody::FormatFileResponse(message.id, result)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::CancelFormat(message_id) => {
        if let Some(token) = context.cancellation_tokens.get_cloned(message_id) {
          token.cancel();
        }
      }
      EditorMessageBody::CanFormat(_) | EditorMessageBody::CanFormatResponse(_, _) | EditorMessageBody::Format(_) | EditorMessageBody::FormatResponse(_, _) => {
        unreachable!("only read with schema version 5");
      }
      EditorMessageBody::FileInfoResponse(_, _)
      | EditorMessageBody::ConfigInfoResponse(_, _)
      | EditorMessageBody::FormatFileResponse(_, _)
      | EditorMessageBody::ShowMessage(_)
      | EditorMessageBody::ShowMessageRequest(_) => {
        send_error_response(context, message.id, anyhow!("CLI cannot handle a message with kind {}.", message.body.as_u32()));
      }
      EditorMessageBody::Unknown(message_kind, _) => {
        send_error_response(context, message.id, anyhow!("Unknown message with kind: {}", message_kind));
      }
    }
  }
}

impl<TEnvironment: Environment> WorkspaceEditorService<TEnvironment> {
  async fn get_file_info(&self, message: FileInfoMessage) -> Result<FileInfoResponseMessage> {
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let Some(scope) = self.resolve_scope(&message.file_path, allow_global_config).await? else {
      return Ok(FileInfoResponseMessage {
        config_file: None,
        can_format: false,
        reason: Some(self.get_no_config_reason(allow_global_config)),
      });
    };
    let file_path = self.canonicalize(message.file_path);
    let notebook_path = message.notebook_path.map(|path| self.canonicalize(path));
    // the editor service only receives a path, so an extensionless file's
    // shebang is resolved from the file on disk
    let reason = self.get_cant_format_reason(&scope, &file_path, notebook_path.as_deref(), None);
    Ok(FileInfoResponseMessage {
      config_file: get_config_file_info(&scope),
      can_format: reason.is_none(),
      reason,
    })
  }

  async fn get_config_info(&self, message: ConfigInfoMessage) -> Result<ConfigInfoResponseMessage> {
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let Some(scope) = self.resolve_scope(&message.file_path, allow_global_config).await? else {
      return Ok(ConfigInfoResponseMessage {
        config_file: None,
        plugins: Vec::new(),
      });
    };
    Ok(ConfigInfoResponseMessage {
      config_file: get_config_file_info(&scope),
      plugins: get_editor_plugin_infos(&scope).await?,
    })
  }

  async fn format_file(self: &Rc<Self>, body: FormatFileMessageBody, token: &Arc<CancellationToken>) -> Result<FormatOutcome> {
    let options: FormatFileOptions = if body.options.is_empty() {
      Default::default()
    } else {
      serde_json::from_slice(&body.options).context("Error deserializing the format options.")?
    };
    let allow_global_config = self.allow_global_config(options.allow_global_config);
    let Some(scope) = self.resolve_scope(&body.file_path, allow_global_config).await? else {
      let reason = self.get_no_config_reason(allow_global_config);
      self.notify_no_config(reason, allow_global_config);
      return Ok(FormatOutcome::NotFormatted(reason));
    };
    let file_path = self.canonicalize(body.file_path);
    let notebook_path = options.notebook_path.map(|path| self.canonicalize(path));
    if let Some(reason) = self.get_cant_format_reason(&scope, &file_path, notebook_path.as_deref(), Some(&body.file_bytes)) {
      return Ok(FormatOutcome::NotFormatted(reason));
    }
    let file_text = String::from_utf8(body.file_bytes).context("The file text isn't valid UTF-8.")?;
    let range = get_byte_range(&file_text, body.start, body.end);

    let _permit = self.concurrency_limiter.acquire().await;
    if token.is_cancelled() {
      // the response isn't sent for a cancelled format
      return Ok(FormatOutcome::NotFormatted(NotFormattedReason::Unchanged));
    }
    let result = scope
      .format_stable(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.clone().into_bytes(),
          range: range.clone(),
          override_config: options.override_config,
          token: token.clone(),
        },
        self.ensure_stable_format,
      )
      .await
      .map_err(|err| anyhow!("{}", dprint_core::plugins::error_to_string(&err)))?;
    let Some(formatted_text) = result else {
      return Ok(FormatOutcome::NotFormatted(NotFormattedReason::Unchanged));
    };
    let formatted_text = String::from_utf8(formatted_text).context("The formatted text isn't valid UTF-8.")?;
    // the editor owns the document's line endings, so match them rather than
    // imposing the plugin's configured newline kind (see #965)
    let formatted_text = normalize_to_source_line_endings(&file_text, formatted_text);
    let formatted_text = match notebook_path {
      Some(_) => trim_formatted_cell_text(&file_text, formatted_text, &range),
      None => formatted_text,
    };
    let edits = get_utf16_edits(&file_text, &formatted_text);
    if edits.is_empty() {
      return Ok(FormatOutcome::NotFormatted(NotFormattedReason::Unchanged));
    }
    Ok(FormatOutcome::Formatted(
      edits
        .into_iter()
        .map(|edit| FormatEdit {
          start: edit.start,
          end: edit.end,
          new_text: edit.new_text,
        })
        .collect(),
    ))
  }

  /// Notifies the user that a file wasn't formatted because no config file
  /// was found for it. This is only done once per session to not annoy people.
  fn notify_no_config(self: &Rc<Self>, reason: NotFormattedReason, allow_global_config: bool) {
    let settings = self.settings.borrow().clone();
    if !settings.show_no_config_notification || self.has_notified_no_config.replace(true) {
      return;
    }
    if is_no_config_notification_dismissed(&self.environment, &settings.workspace_folders) {
      return;
    }
    let message = get_no_config_message(NoConfigMessageOptions {
      use_global_config: allow_global_config,
      has_global_config: reason == NotFormattedReason::GlobalConfigNotAllowed,
      settings_owner: EDITOR_EXTENSION_SETTINGS_OWNER,
    });
    let mut actions = Vec::new();
    if !settings.workspace_folders.is_empty() {
      actions.push(DISMISS_WORKSPACE_ACTION_TITLE);
    }
    actions.push(DISMISS_ACTION_TITLE);
    let service = self.clone();
    // don't wait on this because it only resolves once the user responds
    dprint_core::async_runtime::spawn(async move {
      let Some(action) = service.show_message_request(MessageKind::Info, message, &actions).await else {
        return;
      };
      let dismissal = if action.title == DISMISS_WORKSPACE_ACTION_TITLE {
        NoConfigNotificationDismissal::WorkspaceFolders(settings.workspace_folders)
      } else {
        NoConfigNotificationDismissal::Everywhere
      };
      if let Err(err) = dismiss_no_config_notification(&service.environment, dismissal).await {
        let message = format!("Failed storing to not show the no configuration file notification: {:#}", err);
        log_warn!(service.environment, "{}", message);
        service.show_message(MessageKind::Warning, message);
      }
    });
  }

  /// Shows the message to the user.
  fn show_message(&self, kind: MessageKind, message: String) {
    let data = serde_json::to_vec(&ShowMessageMessage { kind, message }).expect("message should serialize");
    send_response_body(&self.context, EditorMessageBody::ShowMessage(data));
  }

  /// Shows the message to the user with the actions to choose from, which
  /// resolves to the action they chose once they respond.
  async fn show_message_request(&self, kind: MessageKind, message: String, actions: &[&str]) -> Option<MessageAction> {
    let data = ShowMessageRequestMessage {
      kind,
      message,
      actions: actions.iter().map(|title| MessageAction { title: title.to_string() }).collect(),
    };
    let data = serde_json::to_vec(&data).expect("message should serialize");
    let message_id = self.context.id_generator.next();
    let (sender, receiver) = oneshot::channel();
    self.pending_message_requests.store(message_id, sender);
    if self
      .context
      .writer
      .send(EditorMessage {
        id: message_id,
        body: EditorMessageBody::ShowMessageRequest(data),
      })
      .is_err()
    {
      self.pending_message_requests.take(message_id);
      self.context.output_closed.cancel();
      return None;
    }
    // not choosing an action when the editor didn't respond
    receiver.await.ok().flatten()
  }

  /// Gets whether the global config file may be used, which a request or the
  /// settings can allow.
  fn allow_global_config(&self, request_allows: bool) -> bool {
    request_allows || self.settings.borrow().allow_global_config
  }

  /// Resolves the plugins of the config file used for the file, which is
  /// `None` when there isn't one.
  async fn resolve_scope(&self, file_path: &Path, allow_global_config: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let Some(dir_path) = file_path.parent() else {
      bail!("Cannot format non-file path: {}", file_path.display());
    };
    let scope = self.scope_container.resolve_by_path(dir_path, allow_global_config).await?;
    if let Some(scope) = &scope {
      scope.ensure_no_global_config_diagnostics()?;
    }
    Ok(scope)
  }

  /// Gets why there's no config file for a file that doesn't have one in an
  /// ancestor directory.
  fn get_no_config_reason(&self, allow_global_config: bool) -> NotFormattedReason {
    if !allow_global_config && matches!(resolve_global_config_path_and_text(&self.environment), Ok(Some(_))) {
      NotFormattedReason::GlobalConfigNotAllowed
    } else {
      NotFormattedReason::NoConfigFile
    }
  }

  /// Gets why the scope can't format the file, which is `None` when it can.
  fn get_cant_format_reason(
    &self,
    scope: &PluginsScope<TEnvironment>,
    file_path: &Path,
    notebook_path: Option<&Path>,
    file_bytes: Option<&[u8]>,
  ) -> Option<NotFormattedReason> {
    let can_format = match notebook_path {
      // the cli formats a notebook's cells when a plugin (the jupyter plugin) formats
      // the notebook, so only format a cell when the notebook would be formatted
      Some(notebook_path) => {
        !scope.plugin_name_maps.get_plugin_names_from_file_path(notebook_path).into_names().is_empty() && scope.can_format_for_editor(notebook_path, None)
      }
      None => scope.can_format_for_editor(file_path, file_bytes),
    };
    if !can_format {
      return Some(NotFormattedReason::NotMatched);
    }
    let has_plugin = match file_bytes {
      Some(file_bytes) => !scope
        .plugin_name_maps
        .get_plugin_names_from_file_path_and_bytes(file_path, file_bytes)
        .is_empty(),
      None => !get_plugin_names_for_file_on_disk(&scope.plugin_name_maps, file_path, &self.environment).is_empty(),
    };
    if has_plugin { None } else { Some(NotFormattedReason::NoPlugin) }
  }

  /// Canonicalizes the path so that it matches the canonicalized config paths
  /// (ex. for the includes and excludes).
  fn canonicalize(&self, path: PathBuf) -> PathBuf {
    self.environment.canonicalize_maybe_not_exists(&path).map(|p| p.into_path_buf()).unwrap_or(path)
  }
}

fn get_config_file_info<TEnvironment: Environment>(scope: &PluginsScope<TEnvironment>) -> Option<ConfigFileInfo> {
  let config = scope.config.as_ref()?;
  Some(ConfigFileInfo {
    path: config.source.display(),
    is_global: config.is_global,
  })
}

/// Gets whether the message can only be handled once the editor service is
/// initialized.
fn requires_initialize(body: &EditorMessageBody) -> bool {
  matches!(
    body,
    EditorMessageBody::UpdateSettings(_) | EditorMessageBody::FileInfo(_) | EditorMessageBody::ConfigInfo(_) | EditorMessageBody::FormatFile(_)
  )
}

/// Converts the UTF-16 offsets of a range to format to bytes, which is `None`
/// for the whole text. The offsets are clamped to the text.
fn get_byte_range(text: &str, start: u32, end: u32) -> FormatRange {
  let end = utf16_to_byte_offset(text, end);
  let start = std::cmp::min(utf16_to_byte_offset(text, start), end);
  if start == 0 && end == text.len() { None } else { Some(start..end) }
}

/// Gets the byte offset of a UTF-16 offset, which is the start of the
/// character when the offset is in the middle of one.
fn utf16_to_byte_offset(text: &str, offset: u32) -> usize {
  let mut utf16_offset = 0;
  for (byte_index, c) in text.char_indices() {
    utf16_offset += c.len_utf16() as u32;
    if utf16_offset > offset {
      return byte_index;
    }
  }
  text.len()
}

#[cfg(test)]
mod test {
  use std::cell::RefCell;
  use std::io::Read;
  use std::io::Write;
  use std::path::Path;
  use std::path::PathBuf;
  use std::rc::Rc;

  use anyhow::Result;
  use anyhow::anyhow;
  use dprint_core::async_runtime::FutureExt;
  use dprint_core::async_runtime::LocalBoxFuture;
  use dprint_core::communication::IdGenerator;
  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::RcIdStore;
  use dprint_core::communication::SingleThreadMessageWriter;
  use dprint_core::configuration::ConfigKeyMap;
  use pretty_assertions::assert_eq;
  use tokio::sync::oneshot;

  use crate::commands::no_config::NoConfigNotificationDismissal;
  use crate::commands::no_config::dismiss_no_config_notification;
  use crate::commands::no_config::is_no_config_notification_dismissed;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;

  use super::super::messages::ClientInfo;
  use super::super::messages::ConfigFileInfo;
  use super::super::messages::ConfigInfoMessage;
  use super::super::messages::ConfigInfoResponseMessage;
  use super::super::messages::EditorMessage;
  use super::super::messages::EditorMessageBody;
  use super::super::messages::EditorSettings;
  use super::super::messages::FileInfoMessage;
  use super::super::messages::FileInfoResponseMessage;
  use super::super::messages::FormatEdit;
  use super::super::messages::FormatFileMessageBody;
  use super::super::messages::FormatFileOptions;
  use super::super::messages::FormatFileResult;
  use super::super::messages::FormattedMessage;
  use super::super::messages::InitializeMessage;
  use super::super::messages::MessageAction;
  use super::super::messages::MessageKind;
  use super::super::messages::NotFormattedMessage;
  use super::super::messages::NotFormattedReason;
  use super::super::messages::ShowMessageMessage;
  use super::super::messages::ShowMessageRequestMessage;
  use super::super::messages::ShowMessageRequestResponseMessage;

  #[test]
  fn should_format_files_with_the_config_file_of_each() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "a" }"#);
      })
      .with_local_config("/b/dprint.json", |c| {
        c.add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "b" }"#)
          .add_excludes("excluded.txt");
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.file_info(file_info("/a/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/a/dprint.json")),
            can_format: true,
            reason: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: true,
            reason: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/excluded.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: false,
            reason: Some(NotFormattedReason::NotMatched),
          }
        );
        assert_eq!(
          client.file_info(file_info("/a/file.other")).await.unwrap().reason,
          Some(NotFormattedReason::NoPlugin)
        );
        assert_eq!(
          client.file_info(file_info("/c/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: None,
            can_format: false,
            reason: Some(NotFormattedReason::NoConfigFile),
          }
        );

        assert_eq!(client.format("/a/file.txt", "testing", None).await.unwrap(), formatted("testing_a"));
        assert_eq!(client.format("/b/file.txt", "testing", None).await.unwrap(), formatted("testing_b"));
        assert_eq!(
          client.format("/a/file.txt", "testing_a", None).await.unwrap(),
          not_formatted(NotFormattedReason::Unchanged)
        );
        assert_eq!(
          client.format("/b/excluded.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NotMatched)
        );
        assert_eq!(
          client.format("/c/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert_eq!(client.format("/a/file.txt", "should_error", None).await.unwrap_err().to_string(), "Did error.");

        let config_info = client
          .config_info(ConfigInfoMessage {
            file_path: PathBuf::from("/b/dprint.json"),
            allow_global_config: false,
          })
          .await
          .unwrap();
        assert_eq!(config_info.config_file, Some(local_config("/b/dprint.json")));
        assert_eq!(
          config_info.plugins.iter().map(|plugin| plugin.name.as_str()).collect::<Vec<_>>(),
          vec!["test-plugin"]
        );
        assert_eq!(config_info.plugins[0].file_extensions, vec!["txt"]);
      }
      .boxed_local()
    });
    // the plugin is only compiled once for both config files
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_use_global_config_when_allowed() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "global" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.file_info(file_info("/dir/file.txt")).await.unwrap().reason,
          Some(NotFormattedReason::GlobalConfigNotAllowed)
        );
        let info = client
          .file_info(FileInfoMessage {
            allow_global_config: true,
            ..file_info("/dir/file.txt")
          })
          .await
          .unwrap();
        assert!(info.can_format);
        assert!(info.config_file.unwrap().is_global);

        let options = FormatFileOptions {
          allow_global_config: true,
          ..Default::default()
        };
        assert_eq!(
          client.format("/dir/file.txt", "testing", Some(options.clone())).await.unwrap(),
          formatted("testing_global")
        );
        let mut override_config = ConfigKeyMap::new();
        override_config.insert("ending".to_string(), "override".into());
        assert_eq!(
          client
            .format("/dir/file.txt", "testing", Some(FormatFileOptions { override_config, ..options }))
            .await
            .unwrap(),
          formatted("testing_override")
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_use_global_config_when_the_setting_allows_it() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "global" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        client
          .update_settings(EditorSettings {
            allow_global_config: true,
            ..Default::default()
          })
          .await
          .unwrap();
        assert_eq!(client.format("/dir/file.txt", "testing", None).await.unwrap(), formatted("testing_global"));
        assert!(client.take_message_requests().is_empty());
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_notify_once_when_no_config_file() {
    let environment = TestEnvironmentBuilder::new().build();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          client
            .update_settings(EditorSettings {
              workspace_folders: vec![PathBuf::from("/project")],
              ..Default::default()
            })
            .await
            .unwrap();
          client.set_message_request_answer(Some("Don't show again"));
          assert_eq!(
            client.format("/project/file.txt", "testing", None).await.unwrap(),
            not_formatted(NotFormattedReason::NoConfigFile)
          );
          assert_eq!(
            client.format("/project/file.txt", "testing", None).await.unwrap(),
            not_formatted(NotFormattedReason::NoConfigFile)
          );
          assert_eq!(
            client.wait_for_message_requests(1).await,
            vec![ShowMessageRequestMessage {
              kind: MessageKind::Info,
              message: "No dprint configuration file found. Run \"dprint init\" in your project to create one.".to_string(),
              actions: vec![action("Don't show in this workspace"), action("Don't show again")],
            }]
          );
          // the dismissal is stored after the response is received
          for _ in 0..100 {
            if is_no_config_notification_dismissed(&environment, &[]) {
              break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
          }
          assert!(is_no_config_notification_dismissed(&environment, &[]));
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_notify_when_global_config_not_allowed() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.format("/project/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::GlobalConfigNotAllowed)
        );
        assert_eq!(
          client.wait_for_message_requests(1).await,
          vec![ShowMessageRequestMessage {
            kind: MessageKind::Info,
            message: concat!(
              "No dprint configuration file found. Run \"dprint init\" in your project to create one ",
              "or enable the \"useGlobalConfig\" setting of the dprint extension to use your global one."
            )
            .to_string(),
            actions: vec![action("Don't show again")],
          }]
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_not_notify_when_no_config_file_after_dismissed() {
    let environment = TestEnvironmentBuilder::new().build();
    environment
      .run_in_runtime(dismiss_no_config_notification(&environment, NoConfigNotificationDismissal::Everywhere))
      .unwrap();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.format("/project/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert!(client.take_message_requests().is_empty());
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_not_notify_when_no_config_file_and_setting_disabled() {
    let environment = TestEnvironmentBuilder::new().build();

    run_session(&environment, &[], |client| {
      async move {
        client
          .update_settings(EditorSettings {
            show_no_config_notification: false,
            ..Default::default()
          })
          .await
          .unwrap();
        assert_eq!(
          client.format("/project/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert!(client.take_message_requests().is_empty());
        assert!(client.take_messages().is_empty());
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_use_the_config_file_after_it_changes() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "before" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          assert_eq!(client.format("/file.txt", "testing", None).await.unwrap(), formatted("testing_before"));
          let config_text = environment.read_file("/dprint.json").unwrap().replace("before", "after");
          environment.write_file("/dprint.json", &config_text).unwrap();
          assert_eq!(client.format("/file.txt", "testing", None).await.unwrap(), formatted("testing_after"));
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_use_the_config_file_specified_with_config_arg() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/custom.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "specified" }"#);
      })
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "a" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &["--config", "/custom.json"], |client| {
      async move {
        assert_eq!(client.format("/a/file.txt", "testing", None).await.unwrap(), formatted("testing_specified"));
      }
      .boxed_local()
    });
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_respond_with_edits_using_utf16_offsets() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        // the emoji is two UTF-16 code units and four bytes
        let text = "\u{1F389}testing";
        assert_eq!(
          client.format_edits("/file.txt", text, 0..9, None).await.unwrap(),
          Ok(vec![FormatEdit {
            start: 9,
            end: 9,
            new_text: "_formatted".to_string(),
          }])
        );
        // starting in the middle of the emoji starts at the emoji and the end
        // is clamped to the text, so this formats the whole text
        assert_eq!(
          client.format_range("/file.txt", text, 1..100, None).await.unwrap(),
          formatted("\u{1F389}testing_formatted")
        );
        assert_eq!(
          client.format("/file.txt", "testing_formatted", None).await.unwrap(),
          not_formatted(NotFormattedReason::Unchanged)
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_format_notebook_cells() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        // the process plugin stands in for the jupyter plugin
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "formatted\n" }"#)
          .add_config_section("testProcessPlugin", r#"{ "associations": ["**/*.ipynb"] }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        let cell_options = FormatFileOptions {
          notebook_path: Some(PathBuf::from("/dir/notebook.ipynb")),
          ..Default::default()
        };
        // the final newline is trimmed
        assert_eq!(
          client.format("/dir/notebook.txt", "text", Some(cell_options.clone())).await.unwrap(),
          formatted("text_formatted")
        );
        // the cell is only formatted when the plugins handle its language
        assert_eq!(
          client.format("/dir/notebook.py", "text", Some(cell_options)).await.unwrap(),
          not_formatted(NotFormattedReason::NoPlugin)
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_require_initializing_once() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session_without_initializing(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.file_info(file_info("/file.txt")).await.unwrap_err().to_string(),
          "Send an initialize message before message kind 9."
        );
        client.request(EditorMessageBody::IsAlive).await.unwrap();
        client.initialize(Default::default()).await.unwrap();
        assert_eq!(
          client.initialize(Default::default()).await.unwrap_err().to_string(),
          "The editor service was already initialized."
        );
        assert!(client.file_info(file_info("/file.txt")).await.unwrap().can_format);
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_not_start_schema_version_that_does_not_exist() {
    let environment = TestEnvironment::new();
    let err = run_test_cli(vec!["editor-service", "--parent-pid", "1", "--schema-version", "7"], &environment).unwrap_err();
    err.assert_exit_code(10);
    assert!(err.to_string().contains("7 is not in 5..=6"), "{err:#}");
  }

  #[derive(Debug, PartialEq)]
  enum FormatOutcome {
    Formatted(String),
    NotFormatted(NotFormattedReason),
  }

  fn formatted(text: &str) -> FormatOutcome {
    FormatOutcome::Formatted(text.to_string())
  }

  fn not_formatted(reason: NotFormattedReason) -> FormatOutcome {
    FormatOutcome::NotFormatted(reason)
  }

  fn file_info(file_path: &str) -> FileInfoMessage {
    FileInfoMessage {
      file_path: PathBuf::from(file_path),
      notebook_path: None,
      allow_global_config: false,
    }
  }

  fn apply_edits(text: &str, edits: &[FormatEdit]) -> String {
    let mut text = text.encode_utf16().collect::<Vec<_>>();
    // the edits are in order, so applying them from the end keeps the offsets
    // of the earlier ones correct
    for edit in edits.iter().rev() {
      text.splice(edit.start as usize..edit.end as usize, edit.new_text.encode_utf16());
    }
    String::from_utf16(&text).unwrap()
  }

  fn action(title: &str) -> MessageAction {
    MessageAction { title: title.to_string() }
  }

  fn local_config(path: &str) -> ConfigFileInfo {
    ConfigFileInfo {
      path: path.to_string(),
      is_global: false,
    }
  }

  /// Runs the schema version 6 editor service with the client, which is shut
  /// down after the action.
  fn run_session(environment: &TestEnvironment, args: &[&str], action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + Send + 'static) {
    run_session_without_initializing(environment, args, |client| {
      async move {
        client.initialize(Default::default()).await.unwrap();
        action(client).await;
      }
      .boxed_local()
    });
  }

  fn run_session_without_initializing(
    environment: &TestEnvironment,
    args: &[&str],
    action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + Send + 'static,
  ) {
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();
    let result = std::thread::spawn(move || {
      TestEnvironment::new().run_in_runtime(async move {
        let client = Client::new(stdin, stdout);
        action(client.clone()).await;
        client.request(EditorMessageBody::Close).await.unwrap();
      });
    });
    // usually this would be the editor's process id, but this is ok for testing purposes
    let pid = std::process::id().to_string();
    let mut cli_args = vec!["editor-service", "--parent-pid", &pid, "--schema-version", "6"];
    cli_args.extend(args);
    run_test_cli(cli_args, environment).unwrap();
    result.join().unwrap();
  }

  /// An editor using schema version 6.
  #[derive(Clone)]
  struct Client {
    writer: Rc<SingleThreadMessageWriter<EditorMessage>>,
    id_generator: Rc<IdGenerator>,
    pending: RcIdStore<oneshot::Sender<EditorMessageBody>>,
    shown: Rc<RefCell<ShownMessages>>,
  }

  #[derive(Default)]
  struct ShownMessages {
    messages: Vec<ShowMessageMessage>,
    message_requests: Vec<ShowMessageRequestMessage>,
    /// The title of the action to choose for a message request.
    answer: Option<String>,
  }

  impl Client {
    pub fn new(stdin: Box<dyn Write + Send>, stdout: Box<dyn Read + Send>) -> Self {
      let client = Client {
        writer: Rc::new(SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdin))),
        id_generator: Default::default(),
        pending: Default::default(),
        shown: Default::default(),
      };
      let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
      dprint_core::async_runtime::spawn_blocking(move || {
        let mut reader = MessageReader::new(stdout);
        while let Ok(message) = EditorMessage::read(&mut reader, 6) {
          if tx.send(message).is_err() {
            break;
          }
        }
      });
      let pending = client.pending.clone();
      let response_client = client.clone();
      dprint_core::async_runtime::spawn(async move {
        while let Some(message) = rx.recv().await {
          let message_id = match &message.body {
            EditorMessageBody::ShowMessage(data) => {
              response_client.shown.borrow_mut().messages.push(serde_json::from_slice(data).unwrap());
              continue;
            }
            EditorMessageBody::ShowMessageRequest(data) => {
              let mut shown = response_client.shown.borrow_mut();
              shown.message_requests.push(serde_json::from_slice(data).unwrap());
              let response = ShowMessageRequestResponseMessage {
                action: shown.answer.clone().map(|title| MessageAction { title }),
              };
              response_client
                .writer
                .send(EditorMessage {
                  id: response_client.id_generator.next(),
                  body: EditorMessageBody::ShowMessageRequestResponse(message.id, serde_json::to_vec(&response).unwrap()),
                })
                .unwrap();
              continue;
            }
            EditorMessageBody::Success(message_id)
            | EditorMessageBody::Error(message_id, _)
            | EditorMessageBody::FileInfoResponse(message_id, _)
            | EditorMessageBody::ConfigInfoResponse(message_id, _)
            | EditorMessageBody::FormatFileResponse(message_id, _) => *message_id,
            body => panic!("Unexpected message: {body:?}"),
          };
          if let Some(sender) = pending.take(message_id) {
            let _ = sender.send(message.body);
          }
        }
      });
      client
    }

    pub fn set_message_request_answer(&self, title: Option<&str>) {
      self.shown.borrow_mut().answer = title.map(ToString::to_string);
    }

    pub fn take_messages(&self) -> Vec<ShowMessageMessage> {
      std::mem::take(&mut self.shown.borrow_mut().messages)
    }

    pub fn take_message_requests(&self) -> Vec<ShowMessageRequestMessage> {
      std::mem::take(&mut self.shown.borrow_mut().message_requests)
    }

    /// Waits for the service to have sent the number of message requests, which
    /// it sends separately from responding to a message.
    pub async fn wait_for_message_requests(&self, count: usize) -> Vec<ShowMessageRequestMessage> {
      for _ in 0..100 {
        if self.shown.borrow().message_requests.len() >= count {
          break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
      self.take_message_requests()
    }

    pub async fn initialize(&self, settings: EditorSettings) -> Result<()> {
      let message = InitializeMessage {
        client_info: ClientInfo {
          name: "test".to_string(),
          version: None,
        },
        settings,
      };
      self.request(EditorMessageBody::Initialize(serde_json::to_vec(&message)?)).await?;
      Ok(())
    }

    pub async fn update_settings(&self, settings: EditorSettings) -> Result<()> {
      self.request(EditorMessageBody::UpdateSettings(serde_json::to_vec(&settings)?)).await?;
      Ok(())
    }

    pub async fn file_info(&self, message: FileInfoMessage) -> Result<FileInfoResponseMessage> {
      match self.request(EditorMessageBody::FileInfo(serde_json::to_vec(&message)?)).await? {
        EditorMessageBody::FileInfoResponse(_, data) => Ok(serde_json::from_slice(&data)?),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    pub async fn config_info(&self, message: ConfigInfoMessage) -> Result<ConfigInfoResponseMessage> {
      match self.request(EditorMessageBody::ConfigInfo(serde_json::to_vec(&message)?)).await? {
        EditorMessageBody::ConfigInfoResponse(_, data) => Ok(serde_json::from_slice(&data)?),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    pub async fn format(&self, file_path: impl AsRef<Path>, text: &str, options: Option<FormatFileOptions>) -> Result<FormatOutcome> {
      self.format_range(file_path, text, 0..text.encode_utf16().count() as u32, options).await
    }

    /// Formats the text between the UTF-16 offsets and applies the edits.
    pub async fn format_range(
      &self,
      file_path: impl AsRef<Path>,
      text: &str,
      range: std::ops::Range<u32>,
      options: Option<FormatFileOptions>,
    ) -> Result<FormatOutcome> {
      Ok(match self.format_edits(file_path, text, range, options).await? {
        Ok(edits) => FormatOutcome::Formatted(apply_edits(text, &edits)),
        Err(reason) => FormatOutcome::NotFormatted(reason),
      })
    }

    /// Gets the edits that format the text or why it wasn't formatted.
    pub async fn format_edits(
      &self,
      file_path: impl AsRef<Path>,
      text: &str,
      range: std::ops::Range<u32>,
      options: Option<FormatFileOptions>,
    ) -> Result<Result<Vec<FormatEdit>, NotFormattedReason>> {
      let body = EditorMessageBody::FormatFile(FormatFileMessageBody {
        file_path: file_path.as_ref().to_path_buf(),
        start: range.start,
        end: range.end,
        options: match options {
          Some(options) => serde_json::to_vec(&options)?,
          None => Vec::new(),
        },
        file_bytes: text.as_bytes().to_vec(),
      });
      match self.request(body).await? {
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::Formatted(data)) => Ok(Ok(serde_json::from_slice::<FormattedMessage>(&data)?.edits)),
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::NotFormatted(data)) => Ok(Err(serde_json::from_slice::<NotFormattedMessage>(&data)?.reason)),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    /// Sends the message and gets the response, which is an error for an error response.
    pub async fn request(&self, body: EditorMessageBody) -> Result<EditorMessageBody> {
      let message_id = self.id_generator.next();
      let (tx, rx) = oneshot::channel();
      self.pending.store(message_id, tx);
      self.writer.send(EditorMessage { id: message_id, body })?;
      match rx.await? {
        EditorMessageBody::Error(_, data) => Err(anyhow!("{}", String::from_utf8_lossy(&data))),
        body => Ok(body),
      }
    }
  }
}
