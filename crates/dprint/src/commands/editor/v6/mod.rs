//! The editor service of schema version 6, which formats the files of a whole
//! workspace in one process by finding the config file of each file, the same
//! as the language server.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::communication::RcIdStore;
use dprint_core::communication::SingleThreadMessageWriter;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::HostFormatRequest;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::arg_parser::CliArgs;
use crate::commands::lsp::config_completion::ConfigJsonSchemaBuilder;
use crate::commands::lsp::config_completion::PluginSchemaUrl;
use crate::commands::lsp::file_path_to_uri;
use crate::commands::lsp::language::get_language_file_name;
use crate::commands::lsp::notebook::trim_formatted_cell_text;
use crate::commands::lsp::text::RangeFormat;
use crate::commands::lsp::text::get_range_utf16_edits;
use crate::commands::lsp::text::normalize_to_source_line_endings;
use crate::commands::lsp::untitled::is_untitled_uri;
use crate::commands::lsp::uri_to_file_path;
use crate::commands::no_config::DISMISS_ACTION_TITLE;
use crate::commands::no_config::DISMISS_WORKSPACE_ACTION_TITLE;
use crate::commands::no_config::EDITOR_EXTENSION_SETTINGS_OWNER;
use crate::commands::no_config::NoConfigMessageOptions;
use crate::commands::no_config::NoConfigNotificationDismissal;
use crate::commands::no_config::dismiss_no_config_notification;
use crate::commands::no_config::get_no_config_message;
use crate::commands::no_config::is_no_config_notification_dismissed;
use crate::commands::not_formatted_messages::NO_CELL_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NO_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NO_PLUGINS_MESSAGE;
use crate::commands::not_formatted_messages::NOT_MATCHED_MESSAGE;
use crate::commands::not_formatted_messages::RANGE_FORMATTING_NOT_SUPPORTED_MESSAGE;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::commands::scope_container::get_config_file_key;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::resolve_global_config_path_and_text;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::paths::get_plugin_names_for_file_on_disk;
use crate::plugins::PluginConfigDiagnostic;
use crate::plugins::PluginResolver;
use crate::resolution::GetPluginResult;
use crate::resolution::PluginConfigDiagnosticsError;
use crate::resolution::PluginsScope;
use crate::resolution::RangeFormattingSupport;
use crate::utils::LogLevel;
use crate::utils::OutputSink;
use crate::utils::PathSource;
use crate::utils::Semaphore;

use super::AtomicIdGenerator;
use super::EditorContext;
use super::get_editor_plugin_infos;
use super::messages::MessageBody;
use super::new_format_concurrency_limiter;
use super::send_error_response;
use super::send_response_body;
use super::start_reading_messages;

mod config_diagnostics;
mod messages;

use self::config_diagnostics::PluginSectionDiagnostic;
use self::config_diagnostics::get_config_diagnostics;
use self::config_diagnostics::get_config_error_diagnostic;
use self::messages::ClientInfo;
use self::messages::ConfigChangedMessage;
use self::messages::ConfigDiagnostic;
use self::messages::ConfigDiagnosticsMessage;
use self::messages::ConfigFileInfo;
use self::messages::ConfigInfoMessage;
use self::messages::ConfigInfoResponseMessage;
use self::messages::ConfigSchemaMessage;
use self::messages::ConfigSchemaResponseMessage;
use self::messages::DocumentInfo;
use self::messages::DocumentLanguage;
use self::messages::EditorMessage;
use self::messages::EditorMessageBody;
use self::messages::EditorSettings;
use self::messages::FileInfoMessage;
use self::messages::FileInfoResponseMessage;
use self::messages::FormatEdit;
use self::messages::FormatFileMessage;
use self::messages::FormatFileMessageBody;
use self::messages::FormatFileResult;
use self::messages::FormattedMessage;
use self::messages::InitializeMessage;
use self::messages::LogMessage;
use self::messages::LogMessageLevel;
use self::messages::MessageAction;
use self::messages::MessageKind;
use self::messages::NotFormattedMessage;
use self::messages::NotFormattedReason;
use self::messages::ProgressMessage;
use self::messages::ShowMessageMessage;
use self::messages::ShowMessageRequestMessage;
use self::messages::ShowMessageRequestResponseMessage;

struct WorkspaceEditorService<TEnvironment: Environment> {
  environment: TEnvironment,
  context: EditorContext<EditorMessageBody>,
  scope_container: PluginsScopeContainer<TEnvironment>,
  concurrency_limiter: Rc<Semaphore>,
  ensure_stable_format: EnsureStableFormat,
  config_schema_builder: ConfigJsonSchemaBuilder<TEnvironment>,
  /// The log level the editor service was started with, which the `verbose`
  /// setting changes.
  initial_log_level: LogLevel,
  /// The error that was shown to the user for resolving each config file,
  /// which is removed once it resolves.
  config_errors: RefCell<HashMap<String, String>>,
  /// The error finding a config file for each directory, which is removed
  /// once one is found. An error is only shown when no other directory has
  /// it because it's often for an ancestor directory (ex. a config file that
  /// can't be read).
  config_find_errors: RefCell<HashMap<PathBuf, String>>,
  /// The error that was shown to the user for the plugin config diagnostics
  /// of each config file.
  plugin_config_errors: RefCell<HashMap<String, ShownPluginConfigError<TEnvironment>>>,
  /// The config that each config file last resolved to, which is how a
  /// change to a config file it extends is noticed.
  resolved_configs: RefCell<HashMap<String, Rc<ResolvedConfig>>>,
  /// The scope whose config diagnostics were checked for each config file.
  checked_scopes: RefCell<HashMap<String, CheckedScope<TEnvironment>>>,
  /// The config diagnostics that were last sent for each config file uri.
  sent_config_diagnostics: RefCell<HashMap<String, Vec<ConfigDiagnostic>>>,
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
  NotFormatted(NotFormattedMessage),
}

/// The files a document may be formatted as.
struct DocumentFile {
  /// The file paths in order of preference, which are in the same directory
  /// and never empty.
  candidates: Vec<PathBuf>,
  /// The notebook's path when the document is one of its cells.
  notebook_path: Option<PathBuf>,
}

/// The file a document is formatted as.
struct ChosenFile {
  file_path: PathBuf,
  /// Why it can't be formatted, which is `None` when it can.
  cant_format_reason: Option<NotFormattedReason>,
}

/// A scope whose config diagnostics were checked.
struct CheckedScope<TEnvironment: Environment> {
  scope: Weak<PluginsScope<TEnvironment>>,
  /// The text of the config file, which is where the diagnostics are.
  config_text: Rc<str>,
}

/// The error that was shown to the user for the plugin config diagnostics of
/// a config file.
struct ShownPluginConfigError<TEnvironment: Environment> {
  /// The scope that had the diagnostics, so they're shown again once the
  /// config file changes.
  scope: Weak<PluginsScope<TEnvironment>>,
  message: String,
}

pub async fn run_editor_service<TEnvironment: Environment>(
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
    config_schema_builder: ConfigJsonSchemaBuilder::new(environment.clone()),
    initial_log_level: environment.log_level(),
    config_errors: Default::default(),
    config_find_errors: Default::default(),
    plugin_config_errors: Default::default(),
    resolved_configs: Default::default(),
    checked_scopes: Default::default(),
    sent_config_diagnostics: Default::default(),
    client_info: Default::default(),
    settings: Default::default(),
    pending_message_requests: Default::default(),
    has_notified_no_config: Default::default(),
  });
  let mut rx = start_reading_messages::<EditorMessageBody>(environment);

  // what's logged and the progress go to the editor instead of stderr, which
  // are sent right away so they're before the response of the request they're for
  environment.set_output_sink(Arc::new(EditorOutputSink {
    id_generator: service.context.id_generator.clone(),
    writer: Arc::downgrade(&service.context.writer),
    output_closed: service.context.output_closed.clone(),
    percentages: Default::default(),
  }));

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
        anyhow!("Send an initialize message before message kind {}.", message.body.kind()),
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
        service.scope_container.shutdown().await;
        send_response_body(context, EditorMessageBody::Success(message.id));
        // the process exits once this returns
        context.writer.flush().await;
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
            service.set_settings(data.settings);
            *service.client_info.borrow_mut() = Some(client_info);
            send_response_body(context, EditorMessageBody::Success(message.id));
          }
          Err(err) => send_error_response(context, message.id, anyhow!("Error deserializing the initialize message. {:#}", err)),
        }
      }
      EditorMessageBody::UpdateSettings(data) => match serde_json::from_slice(&data) {
        Ok(settings) => {
          service.set_settings(settings);
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
      EditorMessageBody::ConfigSchema(data) => {
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = async {
            let message = serde_json::from_slice(&data).context("Error deserializing the config schema message.")?;
            let response = service.get_config_schema(message).await;
            Ok::<_, anyhow::Error>(serde_json::to_vec(&response)?)
          }
          .await;
          match result {
            Ok(data) => send_response_body(&service.context, EditorMessageBody::ConfigSchemaResponse(message.id, data)),
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
              FormatOutcome::NotFormatted(message) => FormatFileResult::NotFormatted(serde_json::to_vec(&message)?),
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
      EditorMessageBody::FileInfoResponse(_, _)
      | EditorMessageBody::ConfigInfoResponse(_, _)
      | EditorMessageBody::FormatFileResponse(_, _)
      | EditorMessageBody::ShowMessage(_)
      | EditorMessageBody::ShowMessageRequest(_)
      | EditorMessageBody::ConfigSchemaResponse(_, _)
      | EditorMessageBody::Log(_)
      | EditorMessageBody::Progress(_)
      | EditorMessageBody::ConfigDiagnostics(_)
      | EditorMessageBody::ConfigChanged(_) => {
        send_error_response(context, message.id, anyhow!("CLI cannot handle a message with kind {}.", message.body.kind()));
      }
      EditorMessageBody::Unknown(message_kind, _) => {
        send_error_response(context, message.id, anyhow!("Unknown message with kind: {}", message_kind));
      }
    }
  }
}

impl<TEnvironment: Environment> WorkspaceEditorService<TEnvironment> {
  async fn get_file_info(self: &Rc<Self>, message: FileInfoMessage) -> Result<FileInfoResponseMessage> {
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let document_file = self.get_document_file(&message.document)?;
    let is_notebook_cell = document_file.notebook_path.is_some();
    let Some(scope) = self.resolve_scope(&document_file.candidates[0], allow_global_config).await? else {
      let reason = self.get_no_config_reason(allow_global_config);
      return Ok(FileInfoResponseMessage {
        config_file: None,
        can_format: false,
        reason: Some(reason),
        message: get_not_formatted_message(reason, is_notebook_cell, allow_global_config),
      });
    };
    // the editor service only receives a uri, so an extensionless file's
    // shebang is resolved from the file on disk
    let reason = self.choose_file(&scope, document_file, None).cant_format_reason;
    Ok(FileInfoResponseMessage {
      config_file: get_config_file_info(&scope),
      can_format: reason.is_none(),
      reason,
      message: reason.and_then(|reason| get_not_formatted_message(reason, is_notebook_cell, allow_global_config)),
    })
  }

  async fn get_config_info(self: &Rc<Self>, message: ConfigInfoMessage) -> Result<ConfigInfoResponseMessage> {
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let file_path = self.get_uri_file_path(&message.uri)?;
    let Some(scope) = self.resolve_scope(&file_path, allow_global_config).await? else {
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

  async fn get_config_schema(self: &Rc<Self>, message: ConfigSchemaMessage) -> ConfigSchemaResponseMessage {
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let mut plugins = Vec::new();
    for uri in &message.uris {
      let Ok(file_path) = self.get_uri_file_path(uri) else {
        continue;
      };
      // a config file that fails to resolve was shown to the user, so its plugins are only left out
      if let Ok(Some(scope)) = self.resolve_scope(&file_path, allow_global_config).await {
        plugins.extend(scope.plugins.values().map(|plugin| PluginSchemaUrl {
          config_key: plugin.info().config_key.clone(),
          url: plugin.info().config_schema_url.clone(),
        }));
      }
    }
    ConfigSchemaResponseMessage {
      schema: self.config_schema_builder.build(&plugins).await,
    }
  }

  async fn format_file(self: &Rc<Self>, body: FormatFileMessageBody, token: &Arc<CancellationToken>) -> Result<FormatOutcome> {
    let message: FormatFileMessage = serde_json::from_slice(&body.data).context("Error deserializing the format file message.")?;
    let allow_global_config = self.allow_global_config(message.allow_global_config);
    let document_file = self.get_document_file(&message.document)?;
    let is_notebook_cell = document_file.notebook_path.is_some();
    let not_formatted = |reason| {
      Ok(FormatOutcome::NotFormatted(NotFormattedMessage {
        reason,
        message: get_not_formatted_message(reason, is_notebook_cell, allow_global_config),
        diagnostics: Vec::new(),
      }))
    };
    let Some(scope) = self.resolve_scope(&document_file.candidates[0], allow_global_config).await? else {
      let reason = self.get_no_config_reason(allow_global_config);
      self.notify_no_config(reason, allow_global_config, message.show_no_config_notification);
      return not_formatted(reason);
    };
    let chosen_file = self.choose_file(&scope, document_file, Some(&body.file_bytes));
    if let Some(reason) = chosen_file.cant_format_reason {
      return not_formatted(reason);
    }
    let file_path = chosen_file.file_path;
    let file_text = String::from_utf8(body.file_bytes).context("The file text isn't valid UTF-8.")?;
    let range = message.range.and_then(|range| get_byte_range(&file_text, range.start, range.end));
    let range_format = match range {
      Some(range) => match scope.range_formatting_support(&file_path, file_text.as_bytes()) {
        RangeFormattingSupport::Supported => RangeFormat::new(&file_text, Some(range), true),
        RangeFormattingSupport::Unknown => RangeFormat::new(&file_text, Some(range), false),
        RangeFormattingSupport::NotSupported => return not_formatted(NotFormattedReason::RangeFormattingNotSupported),
      },
      None => RangeFormat::new(&file_text, None, true),
    };

    let _permit = self.concurrency_limiter.acquire().await;
    if token.is_cancelled() {
      // the response isn't sent for a cancelled format
      return not_formatted(NotFormattedReason::Unchanged);
    }
    let result = scope
      .format_stable(
        HostFormatRequest {
          file_path,
          file_bytes: file_text.clone().into_bytes(),
          range: range_format.plugin_range.clone(),
          override_config: message.override_config,
          token: token.clone(),
        },
        message.ensure_stable_format.map(EnsureStableFormat).unwrap_or(self.ensure_stable_format),
      )
      .await;
    let formatted_text = match result {
      Ok(Some(formatted_text)) => formatted_text,
      Ok(None) => return not_formatted(NotFormattedReason::Unchanged),
      Err(err) => match err.downcast_ref::<PluginConfigDiagnosticsError>() {
        Some(err) => return Ok(self.get_plugin_config_diagnostics_outcome(&scope, &err.diagnostics)),
        None => bail!("{}", dprint_core::plugins::error_to_string(&err)),
      },
    };
    // this may take a while for a large file, so don't hold up the other messages
    let edits = dprint_core::async_runtime::spawn_blocking(move || {
      let formatted_text = String::from_utf8(formatted_text).context("The formatted text isn't valid UTF-8.")?;
      // the editor owns the document's line endings, so match them rather than
      // imposing the plugin's configured newline kind (see #965)
      let formatted_text = normalize_to_source_line_endings(&file_text, formatted_text);
      let formatted_text = if is_notebook_cell {
        trim_formatted_cell_text(&file_text, formatted_text, &range_format.cell_range())
      } else {
        formatted_text
      };
      Ok::<_, anyhow::Error>(get_range_utf16_edits(&file_text, &formatted_text, &range_format.edits_range))
    })
    .await??;
    if edits.is_empty() {
      return not_formatted(NotFormattedReason::Unchanged);
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

  /// Gets the outcome of a file that wasn't formatted because of problems with
  /// the configuration of a plugin, which are also shown to the user once for
  /// the config file until it changes because formatting doesn't log them
  /// again.
  fn get_plugin_config_diagnostics_outcome(&self, scope: &Rc<PluginsScope<TEnvironment>>, diagnostics: &[PluginConfigDiagnostic]) -> FormatOutcome {
    let config_file = scope.config.as_ref().map(|config| config.source.display()).unwrap_or_default();
    let mut message = format!("dprint did not format the file because of errors in the configuration of {}:", config_file);
    for diagnostic in diagnostics {
      message.push('\n');
      message.push_str(&diagnostic.to_string());
    }
    let shown = ShownPluginConfigError {
      scope: Rc::downgrade(scope),
      message: message.clone(),
    };
    let previous = self.plugin_config_errors.borrow_mut().insert(config_file, shown);
    let was_shown = previous.is_some_and(|previous| previous.scope.ptr_eq(&Rc::downgrade(scope)) && previous.message == message);
    if !was_shown {
      self.show_message(MessageKind::Error, message.clone());
    }
    // the diagnostics are where they are in the config file's text
    let config_text = self
      .checked_scopes
      .borrow()
      .values()
      .find(|checked| checked.scope.ptr_eq(&Rc::downgrade(scope)))
      .map(|checked| checked.config_text.clone());
    let plugin_diagnostics = diagnostics
      .iter()
      .map(|diagnostic| PluginSectionDiagnostic {
        config_key: scope
          .plugins
          .values()
          .find(|plugin| plugin.name() == diagnostic.plugin_name)
          .map(|plugin| plugin.info().config_key.as_str())
          .unwrap_or_default(),
        diagnostic,
      })
      .collect::<Vec<_>>();
    FormatOutcome::NotFormatted(NotFormattedMessage {
      reason: NotFormattedReason::ConfigDiagnostics,
      message: Some(message),
      diagnostics: get_config_diagnostics(config_text.as_deref(), &[], &plugin_diagnostics),
    })
  }

  /// Notifies the user that a file wasn't formatted because no config file
  /// was found for it. This is only done once per session to not annoy people.
  fn notify_no_config(self: &Rc<Self>, reason: NotFormattedReason, allow_global_config: bool, show: Option<bool>) {
    let settings = self.settings.borrow().clone();
    if !show.unwrap_or(settings.show_no_config_notification) || self.has_notified_no_config.get() {
      return;
    }
    let workspace_folders = get_workspace_folder_paths(&settings);
    // the workspace folders may change to ones where it wasn't dismissed
    if is_no_config_notification_dismissed(&self.environment, &workspace_folders) {
      return;
    }
    self.has_notified_no_config.set(true);
    let message = get_no_config_message(NoConfigMessageOptions {
      use_global_config: allow_global_config,
      has_global_config: reason == NotFormattedReason::GlobalConfigNotAllowed,
      settings_owner: EDITOR_EXTENSION_SETTINGS_OWNER,
    });
    let mut actions = Vec::new();
    if !workspace_folders.is_empty() {
      actions.push(DISMISS_WORKSPACE_ACTION_TITLE);
    }
    actions.push(DISMISS_ACTION_TITLE);
    let service = self.clone();
    // don't wait on this because it only resolves once the user responds
    dprint_core::async_runtime::spawn(async move {
      let Some(action) = service.show_message_request(MessageKind::Info, message, &actions).await else {
        return;
      };
      let dismissal = match action.title.as_str() {
        DISMISS_WORKSPACE_ACTION_TITLE => NoConfigNotificationDismissal::WorkspaceFolders(workspace_folders),
        DISMISS_ACTION_TITLE => NoConfigNotificationDismissal::Everywhere,
        // the editor responded with an action that wasn't offered
        _ => return,
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

  fn set_settings(&self, settings: EditorSettings) {
    self.environment.set_log_level(match settings.verbose {
      Some(true) => LogLevel::Debug,
      Some(false) if self.initial_log_level == LogLevel::Debug => LogLevel::Info,
      _ => self.initial_log_level,
    });
    *self.settings.borrow_mut() = settings;
  }

  /// Gets whether the global config file may be used, which a request or the
  /// settings can allow.
  fn allow_global_config(&self, request_allows: bool) -> bool {
    request_allows || self.settings.borrow().allow_global_config
  }

  /// Resolves the plugins of the config file used for the file, which is
  /// `None` when there isn't one.
  async fn resolve_scope(self: &Rc<Self>, file_path: &Path, allow_global_config: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let Some(dir_path) = file_path.parent() else {
      bail!("Cannot format non-file path: {}", file_path.display());
    };
    let config_file = match self.scope_container.find_config_file(dir_path, allow_global_config) {
      Ok(config_file) => {
        self.config_find_errors.borrow_mut().remove(dir_path);
        config_file
      }
      Err(err) => {
        self.show_config_find_error(dir_path, &err);
        return Err(err);
      }
    };
    let Some(config_file) = config_file else {
      return Ok(None);
    };
    let key = get_config_file_key(&config_file);
    let scope = match self.scope_container.resolve_config_file(&config_file).await {
      Ok(scope) => scope,
      Err(err) => {
        self.show_config_error(key.clone(), &err);
        self.checked_scopes.borrow_mut().remove(&key);
        self.send_config_diagnostics(&config_file.source, vec![get_config_error_diagnostic(Some(&config_file.content), &err)]);
        return Err(err);
      }
    };
    self.check_config_diagnostics(&key, &config_file, &scope);
    if let Err(err) = scope.ensure_no_global_config_diagnostics() {
      let err = anyhow::Error::from(err);
      self.show_config_error(key, &err);
      return Err(err);
    }
    self.config_errors.borrow_mut().remove(&key);
    self.notify_if_config_changed(key, &config_file, &scope);
    Ok(Some(scope))
  }

  /// Sends the problems with the config file to the editor, which is done
  /// once for each time it resolves to a new scope. The problems with the
  /// configuration of its plugins are only known once they're set up, so
  /// they're sent once that's done.
  fn check_config_diagnostics(self: &Rc<Self>, key: &str, config_file: &ResolvedConfigPathWithText, scope: &Rc<PluginsScope<TEnvironment>>) {
    let weak_scope = Rc::downgrade(scope);
    let config_text: Rc<str> = config_file.content.as_str().into();
    let checked_scope = CheckedScope {
      scope: weak_scope.clone(),
      config_text: config_text.clone(),
    };
    let previous = self.checked_scopes.borrow_mut().insert(key.to_string(), checked_scope);
    if previous.is_some_and(|previous| previous.scope.ptr_eq(&weak_scope)) {
      return;
    }
    self.send_config_diagnostics(
      &config_file.source,
      get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &[]),
    );
    let service = self.clone();
    let key = key.to_string();
    let source = config_file.source.clone();
    let scope = scope.clone();
    dprint_core::async_runtime::spawn(async move {
      let mut plugin_diagnostics = Vec::new();
      for plugin in scope.plugins.values() {
        // a plugin that fails to be set up is reported when it formats
        if let Ok(GetPluginResult::HadDiagnostics(diagnostics)) = plugin.get_or_create_checking_config_diagnostics(&service.environment).await {
          plugin_diagnostics.extend(diagnostics.into_iter().map(|diagnostic| (plugin.info().config_key.clone(), diagnostic)));
        }
      }
      if plugin_diagnostics.is_empty() {
        return;
      }
      // the config file may have changed in the meantime
      let is_current = service
        .checked_scopes
        .borrow()
        .get(&key)
        .is_some_and(|checked| checked.scope.ptr_eq(&Rc::downgrade(&scope)));
      if !is_current {
        return;
      }
      let plugin_diagnostics = plugin_diagnostics
        .iter()
        .map(|(config_key, diagnostic)| PluginSectionDiagnostic { config_key, diagnostic })
        .collect::<Vec<_>>();
      service.send_config_diagnostics(
        &source,
        get_config_diagnostics(Some(&config_text), scope.global_config_diagnostics(), &plugin_diagnostics),
      );
    });
  }

  /// Sends the problems with the config file to the editor when they're
  /// different from the ones that were sent before.
  fn send_config_diagnostics(&self, source: &PathSource, diagnostics: Vec<ConfigDiagnostic>) {
    let Some(uri) = path_source_to_uri(source) else {
      return;
    };
    let mut sent_config_diagnostics = self.sent_config_diagnostics.borrow_mut();
    let previous = sent_config_diagnostics.get(uri.as_str());
    // there's nothing to clear when nothing was sent
    if previous == Some(&diagnostics) || previous.is_none() && diagnostics.is_empty() {
      return;
    }
    sent_config_diagnostics.insert(uri.as_str().to_string(), diagnostics.clone());
    drop(sent_config_diagnostics);
    let data = serde_json::to_vec(&ConfigDiagnosticsMessage { uri, diagnostics }).expect("message should serialize");
    send_response_body(&self.context, EditorMessageBody::ConfigDiagnostics(data));
  }

  /// Tells the editor when the config file resolves to a different config
  /// than before because what it formats may have changed in a way the editor
  /// can't see (ex. a config file that it extends changed).
  fn notify_if_config_changed(&self, key: String, config_file: &ResolvedConfigPathWithText, scope: &PluginsScope<TEnvironment>) {
    let Some(config) = &scope.config else {
      return;
    };
    let previous = self.resolved_configs.borrow_mut().insert(key, config.clone());
    if previous.is_some_and(|previous| previous != *config)
      && let Some(uri) = path_source_to_uri(&config_file.source)
    {
      let data = serde_json::to_vec(&ConfigChangedMessage { uri }).expect("message should serialize");
      send_response_body(&self.context, EditorMessageBody::ConfigChanged(data));
    }
  }

  /// Shows the user the error resolving the config file, which is only done
  /// once until it resolves because the editor makes requests without the
  /// user doing anything (ex. when a file is opened).
  fn show_config_error(&self, key: String, err: &anyhow::Error) {
    let message = format!("{:#}", err);
    let previous_message = self.config_errors.borrow_mut().insert(key, message.clone());
    if previous_message.as_ref() != Some(&message) {
      self.show_message(MessageKind::Error, message);
    }
  }

  /// Shows the user the error finding a config file for the directory, which
  /// is only done when no other directory has the error.
  fn show_config_find_error(&self, dir_path: &Path, err: &anyhow::Error) {
    let message = format!("{:#}", err);
    let mut config_find_errors = self.config_find_errors.borrow_mut();
    let was_shown = config_find_errors.values().any(|shown_message| *shown_message == message);
    config_find_errors.insert(dir_path.to_path_buf(), message.clone());
    drop(config_find_errors);
    if !was_shown {
      self.show_message(MessageKind::Error, message);
    }
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
    if scope.plugins.is_empty() {
      return Some(NotFormattedReason::NoPlugins);
    }
    match notebook_path {
      // the cli formats a notebook's cells when a plugin (the jupyter plugin) formats
      // the notebook, so only format a cell when the notebook would be formatted
      Some(notebook_path) => {
        if !scope.can_format_for_editor(notebook_path, None) {
          return Some(NotFormattedReason::NotMatched);
        }
        if scope.plugin_name_maps.get_plugin_names_from_file_path(notebook_path).into_names().is_empty() {
          return Some(NotFormattedReason::NoPlugin);
        }
      }
      None => {
        if !scope.can_format_for_editor(file_path, file_bytes) {
          return Some(NotFormattedReason::NotMatched);
        }
      }
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

  /// Gets the files the document may be formatted as.
  fn get_document_file(&self, document: &DocumentInfo) -> Result<DocumentFile> {
    if let Some(notebook_uri) = &document.notebook_uri {
      // the cli only formats notebooks on the file system
      let Some(notebook_path) = uri_to_file_path(notebook_uri) else {
        bail!("Cannot format cell of a notebook that is not on the file system: {}", notebook_uri.as_str());
      };
      let Some(dir_path) = notebook_path.parent() else {
        bail!("Cannot format cell of a notebook without a directory: {}", notebook_path.display());
      };
      return Ok(DocumentFile {
        candidates: get_language_file_paths(dir_path, "code_block", document)?,
        notebook_path: Some(notebook_path),
      });
    }
    if is_untitled_uri(&document.uri) {
      return Ok(DocumentFile {
        candidates: get_language_file_paths(&self.get_untitled_dir_path(&document.uri)?, "Untitled", document)?,
        notebook_path: None,
      });
    }
    let file_path = self.get_uri_file_path(&document.uri)?;
    if file_path.parent().is_none() {
      bail!("Cannot format non-file path: {}", file_path.display());
    }
    Ok(DocumentFile {
      candidates: vec![file_path],
      notebook_path: None,
    })
  }

  /// Gets the path of a file for the uri, which is in the directory an
  /// untitled document is formatted in for an untitled uri.
  fn get_uri_file_path(&self, uri: &Uri) -> Result<PathBuf> {
    if is_untitled_uri(uri) {
      return Ok(self.get_untitled_dir_path(uri)?.join("Untitled"));
    }
    match uri_to_file_path(uri) {
      Some(file_path) => Ok(file_path),
      None => bail!(
        "Cannot format document that is not a file, an untitled document or a notebook cell: {}",
        uri.as_str()
      ),
    }
  }

  /// Gets the directory that an untitled document is formatted in, which is
  /// the first workspace folder or otherwise the home directory.
  fn get_untitled_dir_path(&self, uri: &Uri) -> Result<PathBuf> {
    let workspace_folder = get_workspace_folder_paths(&self.settings.borrow()).into_iter().next();
    match workspace_folder.or_else(|| self.environment.get_home_dir().map(|dir| dir.into_path_buf())) {
      Some(dir_path) => Ok(dir_path),
      None => bail!("Cannot format untitled document without a workspace folder or home directory: {}", uri.as_str()),
    }
  }

  /// Chooses the file to format the document as, which is the first one the
  /// scope can format or otherwise the first one.
  fn choose_file(&self, scope: &PluginsScope<TEnvironment>, document_file: DocumentFile, file_bytes: Option<&[u8]>) -> ChosenFile {
    let notebook_path = document_file.notebook_path.map(|path| self.canonicalize(path));
    let mut first = None;
    for candidate in document_file.candidates {
      let file_path = self.canonicalize(candidate);
      let cant_format_reason = self.get_cant_format_reason(scope, &file_path, notebook_path.as_deref(), file_bytes);
      if cant_format_reason.is_none() {
        return ChosenFile { file_path, cant_format_reason };
      }
      first.get_or_insert((file_path, cant_format_reason));
    }
    let (file_path, cant_format_reason) = first.expect("there should be a candidate");
    ChosenFile { file_path, cant_format_reason }
  }

  /// Canonicalizes the path so that it matches the canonicalized config paths
  /// (ex. for the includes and excludes).
  fn canonicalize(&self, path: PathBuf) -> PathBuf {
    self.environment.canonicalize_maybe_not_exists(&path).map(|p| p.into_path_buf()).unwrap_or(path)
  }
}

/// Gets the message to show the user about why a file wasn't formatted (ex.
/// for a command the user ran), which is `None` when it's already formatted.
fn get_not_formatted_message(reason: NotFormattedReason, is_notebook_cell: bool, allow_global_config: bool) -> Option<String> {
  match reason {
    NotFormattedReason::NoConfigFile => Some(get_no_config_message(NoConfigMessageOptions {
      use_global_config: allow_global_config,
      has_global_config: false,
      settings_owner: EDITOR_EXTENSION_SETTINGS_OWNER,
    })),
    NotFormattedReason::GlobalConfigNotAllowed => Some(get_no_config_message(NoConfigMessageOptions {
      use_global_config: false,
      has_global_config: true,
      settings_owner: EDITOR_EXTENSION_SETTINGS_OWNER,
    })),
    NotFormattedReason::NotMatched => Some(NOT_MATCHED_MESSAGE.to_string()),
    NotFormattedReason::NoPlugin if is_notebook_cell => Some(NO_CELL_PLUGIN_MESSAGE.to_string()),
    NotFormattedReason::NoPlugin => Some(NO_PLUGIN_MESSAGE.to_string()),
    NotFormattedReason::NoPlugins => Some(NO_PLUGINS_MESSAGE.to_string()),
    // the message is created with the diagnostics instead
    NotFormattedReason::ConfigDiagnostics => None,
    NotFormattedReason::RangeFormattingNotSupported => Some(RANGE_FORMATTING_NOT_SUPPORTED_MESSAGE.to_string()),
    NotFormattedReason::Unchanged => None,
  }
}

fn get_config_file_info<TEnvironment: Environment>(scope: &PluginsScope<TEnvironment>) -> Option<ConfigFileInfo> {
  let config = scope.config.as_ref()?;
  Some(ConfigFileInfo {
    uri: path_source_to_uri(&config.source)?,
    is_global: config.is_global,
  })
}

/// Gets the uri of a config file.
fn path_source_to_uri(source: &PathSource) -> Option<Uri> {
  match source {
    PathSource::Local(local) => file_path_to_uri(local.path.as_ref()),
    PathSource::Remote(remote) => remote.url.as_str().parse().ok(),
    PathSource::Npm(_) => None,
  }
}

fn get_workspace_folder_paths(settings: &EditorSettings) -> Vec<PathBuf> {
  settings.workspace_folders.iter().filter_map(uri_to_file_path).collect()
}

/// Gets the paths to format a document that isn't a file as, which are in
/// the directory and named by the document's language (ex. `Untitled.ts`).
fn get_language_file_paths(dir_path: &Path, base_name: &str, document: &DocumentInfo) -> Result<Vec<PathBuf>> {
  let Some(language) = &document.language else {
    bail!("Provide the language of the document to format it: {}", document.uri.as_str());
  };
  let file_names = get_language_file_names(base_name, language);
  if file_names.is_empty() {
    bail!("Could not determine a file path to format the document with language: {}", language.id);
  }
  Ok(file_names.into_iter().map(|file_name| dir_path.join(file_name)).collect())
}

/// Gets the file names of the language in order of preference, which are
/// the editor's file extensions and file names followed by the file
/// extension dprint knows for the language.
fn get_language_file_names(base_name: &str, language: &DocumentLanguage) -> Vec<String> {
  // the editor's file names can't be paths
  let is_file_name = |name: &str| !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..";
  let mut file_names = Vec::new();
  let extension_file_names = language
    .file_extensions
    .iter()
    .filter(|extension| extension.len() > 1 && extension.starts_with('.'))
    .map(|extension| format!("{}{}", base_name, extension));
  let editor_file_names = language.file_names.iter().cloned();
  for file_name in extension_file_names
    .chain(editor_file_names)
    .chain(get_language_file_name(base_name, &language.id))
  {
    if is_file_name(&file_name) && !file_names.contains(&file_name) {
      file_names.push(file_name);
    }
  }
  file_names
}

/// Gets whether the message can only be handled once the editor service is
/// initialized.
fn requires_initialize(body: &EditorMessageBody) -> bool {
  matches!(
    body,
    EditorMessageBody::UpdateSettings(_)
      | EditorMessageBody::FileInfo(_)
      | EditorMessageBody::ConfigInfo(_)
      | EditorMessageBody::ConfigSchema(_)
      | EditorMessageBody::FormatFile(_)
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

/// Sends what's logged and the progress to the editor from any thread.
struct EditorOutputSink {
  id_generator: Arc<AtomicIdGenerator>,
  /// Weak so that the writer stops once the editor service does, which the
  /// output sink would otherwise outlive.
  writer: std::sync::Weak<SingleThreadMessageWriter<EditorMessage>>,
  output_closed: CancellationToken,
  /// The last percentage sent for each progress, which is `None` when it's
  /// not known.
  percentages: Mutex<HashMap<usize, Option<u8>>>,
}

impl EditorOutputSink {
  fn send(&self, body: EditorMessageBody) {
    let Some(writer) = self.writer.upgrade() else {
      return;
    };
    let message = EditorMessage {
      id: self.id_generator.next(),
      body,
    };
    if writer.send(message).is_err() {
      self.output_closed.cancel();
    }
  }

  fn send_progress(&self, message: ProgressMessage) {
    self.send(EditorMessageBody::Progress(serde_json::to_vec(&message).expect("message should serialize")));
  }
}

impl OutputSink for EditorOutputSink {
  fn log(&self, level: LogLevel, text: &str) {
    let level = match level {
      LogLevel::Error => LogMessageLevel::Error,
      LogLevel::Warn => LogMessageLevel::Warn,
      LogLevel::Info => LogMessageLevel::Info,
      LogLevel::Debug => LogMessageLevel::Debug,
      LogLevel::Silent => return,
    };
    let message = LogMessage {
      level,
      message: text.to_string(),
    };
    self.send(EditorMessageBody::Log(serde_json::to_vec(&message).expect("message should serialize")));
  }

  fn start_progress(&self, id: usize, title: &str, total_size: usize) {
    let percentage = (total_size > 0).then_some(0);
    self.percentages.lock().insert(id, percentage);
    self.send_progress(ProgressMessage::Begin {
      id,
      title: title.to_string(),
      percentage,
    });
  }

  fn update_progress(&self, id: usize, position: usize, total_size: usize) {
    if total_size == 0 {
      return;
    }
    let percentage = std::cmp::min(100, position as u128 * 100 / total_size as u128) as u8;
    {
      let mut percentages = self.percentages.lock();
      // only send when it changes because this is updated often (ex. for each part of a download)
      match percentages.get_mut(&id) {
        Some(previous) if *previous != Some(percentage) => *previous = Some(percentage),
        _ => return,
      }
    }
    self.send_progress(ProgressMessage::Report { id, percentage });
  }

  fn finish_progress(&self, id: usize) {
    if self.percentages.lock().remove(&id).is_some() {
      self.send_progress(ProgressMessage::End { id });
    }
  }
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

  use crate::commands::lsp::file_path_to_uri;
  use crate::commands::no_config::NoConfigNotificationDismissal;
  use crate::commands::no_config::dismiss_no_config_notification;
  use crate::commands::no_config::is_no_config_notification_dismissed;
  use crate::commands::not_formatted_messages::NO_CELL_PLUGIN_MESSAGE;
  use crate::commands::not_formatted_messages::NO_PLUGIN_MESSAGE;
  use crate::commands::not_formatted_messages::NO_PLUGINS_MESSAGE;
  use crate::commands::not_formatted_messages::NOT_MATCHED_MESSAGE;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;
  use crate::utils::LogLevel;

  use deno_tower_lsp::lsp_types::Position;
  use deno_tower_lsp::lsp_types::Range;
  use deno_tower_lsp::lsp_types::Uri;

  use super::messages::ClientInfo;
  use super::messages::ConfigChangedMessage;
  use super::messages::ConfigDiagnostic;
  use super::messages::ConfigDiagnosticsMessage;
  use super::messages::ConfigFileInfo;
  use super::messages::ConfigInfoMessage;
  use super::messages::ConfigInfoResponseMessage;
  use super::messages::ConfigSchemaMessage;
  use super::messages::ConfigSchemaResponseMessage;
  use super::messages::DocumentInfo;
  use super::messages::DocumentLanguage;
  use super::messages::EditorMessage;
  use super::messages::EditorMessageBody;
  use super::messages::EditorSettings;
  use super::messages::FileInfoMessage;
  use super::messages::FileInfoResponseMessage;
  use super::messages::FormatEdit;
  use super::messages::FormatFileMessage;
  use super::messages::FormatFileMessageBody;
  use super::messages::FormatFileResult;
  use super::messages::FormattedMessage;
  use super::messages::InitializeMessage;
  use super::messages::LogMessage;
  use super::messages::MessageAction;
  use super::messages::MessageKind;
  use super::messages::NotFormattedMessage;
  use super::messages::NotFormattedReason;
  use super::messages::OffsetRange;
  use super::messages::ProgressMessage;
  use super::messages::ShowMessageMessage;
  use super::messages::ShowMessageRequestMessage;
  use super::messages::ShowMessageRequestResponseMessage;

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
            message: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: true,
            reason: None,
            message: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/excluded.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: false,
            reason: Some(NotFormattedReason::NotMatched),
            message: Some(NOT_MATCHED_MESSAGE.to_string()),
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
            message: Some("No dprint configuration file found. Run \"dprint init\" in your project to create one.".to_string()),
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
            uri: uri("/b/dprint.json"),
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
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
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
              workspace_folders: vec![uri("/project")],
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
  fn should_not_notify_when_no_config_file_and_request_turns_off_notification() {
    let environment = TestEnvironmentBuilder::new().build();

    run_session(&environment, &[], |client| {
      async move {
        let options = FormatFileOptions {
          allow_global_config: true,
          show_no_config_notification: Some(false),
          ..Default::default()
        };
        assert_eq!(
          client.format("/project/file.txt", "testing", Some(options)).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        // the notification is still shown for a format that doesn't
        assert_eq!(
          client.format("/project/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert_eq!(client.wait_for_message_requests(1).await.len(), 1);
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_notify_when_no_config_file_and_request_allows_global_config() {
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
        // the request's option is used instead of the setting
        let options = FormatFileOptions {
          allow_global_config: true,
          show_no_config_notification: Some(true),
          ..Default::default()
        };
        assert_eq!(
          client.format("/project/file.txt", "testing", Some(options)).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert_eq!(
          client.wait_for_message_requests(1).await[0].message,
          "No dprint configuration file found. Run \"dprint init\" in your project to create one or \"dprint init --global\" to create a global one."
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_notify_when_no_config_file_in_workspace_folders_that_were_not_dismissed() {
    let environment = TestEnvironmentBuilder::new().build();
    environment
      .run_in_runtime(dismiss_no_config_notification(
        &environment,
        NoConfigNotificationDismissal::WorkspaceFolders(vec![PathBuf::from("/a")]),
      ))
      .unwrap();

    run_session(&environment, &[], |client| {
      async move {
        let settings = |folder: &str| EditorSettings {
          workspace_folders: vec![uri(folder)],
          ..Default::default()
        };
        client.update_settings(settings("/a")).await.unwrap();
        assert_eq!(
          client.format("/a/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert!(client.take_message_requests().is_empty());

        client.update_settings(settings("/b")).await.unwrap();
        assert_eq!(
          client.format("/b/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert_eq!(client.wait_for_message_requests(1).await.len(), 1);
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
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn should_say_why_a_file_was_not_formatted() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_excludes("excluded.txt");
      })
      .with_local_config("/b/dprint.json", |_| {})
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        let message = |reason, message: &str| NotFormattedMessage {
          reason,
          message: Some(message.to_string()),
          diagnostics: Vec::new(),
        };
        assert_eq!(
          client.not_formatted_message("/a/excluded.txt").await,
          message(NotFormattedReason::NotMatched, NOT_MATCHED_MESSAGE)
        );
        assert_eq!(
          client.not_formatted_message("/a/file.other").await,
          message(NotFormattedReason::NoPlugin, NO_PLUGIN_MESSAGE)
        );
        assert_eq!(
          client.not_formatted_message("/b/file.txt").await,
          message(NotFormattedReason::NoPlugins, NO_PLUGINS_MESSAGE)
        );
        assert_eq!(
          client.not_formatted_message("/c/file.txt").await,
          message(
            NotFormattedReason::NoConfigFile,
            "No dprint configuration file found. Run \"dprint init\" in your project to create one."
          )
        );
        assert_eq!(
          client.file_info(file_info("/a/excluded.txt")).await.unwrap().message.as_deref(),
          Some(NOT_MATCHED_MESSAGE)
        );
        assert_eq!(client.file_info(file_info("/a/file.txt")).await.unwrap().message, None);
      }
      .boxed_local()
    });
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn should_provide_the_config_schema_with_the_plugin_schemas() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    environment.add_remote_file(
      "https://plugins.dprint.dev/test/schema.json",
      br##"{
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://plugins.dprint.dev/test/schema.json",
        "type": "object",
        "properties": {
          "ending": { "$ref": "#/definitions/ending" },
          "other": { "$ref": "https://plugins.dprint.dev/test/schema.json#/definitions/ending" }
        },
        "additionalProperties": false,
        "definitions": { "ending": { "type": "string" } }
      }"##,
    );

    run_session(&environment, &[], |client| {
      async move {
        let schema = client.config_schema(&["/a/dprint.json"]).await.unwrap();
        // the plugin's schema is in the definitions with its references changed to point there
        // and the properties the cli handles for every plugin added, which its "additionalProperties"
        // would otherwise disallow
        let mut properties = serde_json::json!({
          "ending": { "$ref": "#/definitions/plugin-test-plugin/definitions/ending" },
          "other": { "$ref": "#/definitions/plugin-test-plugin/definitions/ending" }
        });
        for (key, value) in schema["additionalProperties"]["properties"].as_object().unwrap() {
          properties[key] = value.clone();
        }
        assert_eq!(
          schema["definitions"]["plugin-test-plugin"],
          serde_json::json!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
            "definitions": { "ending": { "type": "string" } }
          })
        );
        assert!(schema["definitions"]["plugin-test-plugin"]["properties"]["associations"].is_object());
        assert_eq!(
          schema["properties"]["test-plugin"],
          serde_json::json!({ "$ref": "#/definitions/plugin-test-plugin" })
        );
        assert_eq!(schema["properties"]["plugins"]["type"], "array");

        // there are no plugins without a config file
        let schema = client.config_schema(&["/c/dprint.json"]).await.unwrap();
        assert!(schema["properties"].get("test-plugin").is_none());
      }
      .boxed_local()
    });
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn should_ensure_a_stable_format_when_requested() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        // formats once by default
        assert_eq!(
          client.format("/file.txt", "unstable_fmt_true", None).await.unwrap(),
          formatted("unstable_fmt_false_formatted")
        );
        let options = FormatFileOptions {
          ensure_stable_format: Some(true),
          ..Default::default()
        };
        assert_eq!(
          client.format("/file.txt", "unstable_fmt_true", Some(options)).await.unwrap_err().to_string(),
          "Formatting not stable. Bailed after 5 tries. This indicates a bug in the plugin where it formats the file differently each time."
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_change_the_log_level_with_the_verbose_setting() {
    let environment = TestEnvironmentBuilder::new().build();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          let verbose = |verbose| EditorSettings { verbose, ..Default::default() };
          client.update_settings(verbose(Some(true))).await.unwrap();
          assert_eq!(environment.log_level(), LogLevel::Debug);
          client.update_settings(verbose(Some(false))).await.unwrap();
          assert_eq!(environment.log_level(), LogLevel::Info);
          client.update_settings(verbose(Some(true))).await.unwrap();
          // back to what it started with
          client.update_settings(verbose(None)).await.unwrap();
          assert_eq!(environment.log_level(), LogLevel::Info);
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_send_what_is_logged_for_a_request_before_its_response() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        client
          .update_settings(EditorSettings {
            verbose: Some(true),
            ..Default::default()
          })
          .await
          .unwrap();
        for _ in 0..10 {
          client.take_logs();
          assert_eq!(client.format("/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
          let logs = client.take_logs();
          assert!(logs.iter().any(|log| log.starts_with("debug: Host formatting")), "{logs:?}");
        }
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_show_a_config_error_once_until_it_resolves() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    let config_text = environment.read_file("/a/dprint.json").unwrap();
    environment.write_file("/a/dprint.json", "{").unwrap();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          // for each file of the config file
          assert!(client.format("/a/file.txt", "text", None).await.is_err());
          assert!(client.format("/a/sub/file.txt", "text", None).await.is_err());
          let messages = client.take_messages();
          assert_eq!(messages.len(), 1);
          assert_eq!(messages[0].kind, MessageKind::Error);
          assert!(messages[0].message.contains("/a/dprint.json"), "{}", messages[0].message);

          // shown again after it resolved, which a file in any of its directories shows
          environment.write_file("/a/dprint.json", &config_text).unwrap();
          assert_eq!(client.format("/a/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
          environment.write_file("/a/dprint.json", "{").unwrap();
          assert!(client.format("/a/sub/file.txt", "text", None).await.is_err());
          assert_eq!(client.take_messages().len(), 1);
        }
        .boxed_local()
      }
    });
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
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
  fn should_not_dismiss_no_config_notification_without_an_offered_action() {
    // an action that wasn't offered and the editor failing to show the message
    for (answer, answer_with_error) in [("Something else", false), ("Don't show again", true)] {
      let environment = TestEnvironmentBuilder::new().build();
      run_session(&environment, &[], {
        let environment = environment.clone();
        move |client| {
          async move {
            client.set_message_request_answer(Some(answer));
            client.set_message_request_error(answer_with_error);
            assert_eq!(
              client.format("/project/file.txt", "testing", None).await.unwrap(),
              not_formatted(NotFormattedReason::NoConfigFile)
            );
            assert_eq!(client.wait_for_message_requests(1).await.len(), 1);
            client.request(EditorMessageBody::IsAlive).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(!is_no_config_notification_dismissed(&environment, &[]));
          }
          .boxed_local()
        }
      });
    }
  }

  #[test]
  fn should_show_an_error_finding_a_config_file_once() {
    let environment = TestEnvironmentBuilder::new().build();
    // reading the config file fails
    environment.mk_dir_all("/a/dprint.json").unwrap();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          // for each directory with the error, including after finding no config file for another directory
          assert!(client.format("/a/file.txt", "text", None).await.is_err());
          assert!(client.format("/a/sub/file.txt", "text", None).await.is_err());
          assert_eq!(
            client.format("/b/file.txt", "text", None).await.unwrap(),
            not_formatted(NotFormattedReason::NoConfigFile)
          );
          assert!(client.format("/a/file.txt", "text", None).await.is_err());
          let messages = client.take_messages();
          assert_eq!(messages.len(), 1, "{messages:?}");
          assert_eq!(messages[0].kind, MessageKind::Error);

          // shown again once the directories that had it no longer do
          environment.remove_dir_all("/a/dprint.json").unwrap();
          for file_path in ["/a/file.txt", "/a/sub/file.txt"] {
            assert_eq!(
              client.format(file_path, "text", None).await.unwrap(),
              not_formatted(NotFormattedReason::NoConfigFile)
            );
          }
          environment.mk_dir_all("/a/dprint.json").unwrap();
          assert!(client.format("/a/file.txt", "text", None).await.is_err());
          assert_eq!(client.take_messages().len(), 1);
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_say_why_a_file_was_not_formatted_because_of_plugin_config_diagnostics() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "non-existent": 1 }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          let expected_message = concat!(
            "dprint did not format the file because of errors in the configuration of /dprint.json:\n",
            "[test-plugin]: Unknown property in configuration (non-existent)"
          );
          let expected = NotFormattedMessage {
            reason: NotFormattedReason::ConfigDiagnostics,
            message: Some(expected_message.to_string()),
            diagnostics: vec![ConfigDiagnostic {
              plugin_name: Some("test-plugin".to_string()),
              property_name: Some("non-existent".to_string()),
              range: Some(Range::new(Position::new(1, 19), Position::new(1, 33))),
              message: "Unknown property in configuration".to_string(),
            }],
          };
          assert_eq!(client.not_formatted_message("/file.txt").await, expected);
          // only shown once for the config file
          assert_eq!(client.not_formatted_message("/sub/file.txt").await, expected);
          let shown = vec![ShowMessageMessage {
            kind: MessageKind::Error,
            message: expected_message.to_string(),
          }];
          assert_eq!(client.take_messages(), shown);

          // shown again once the config file changes even when the diagnostics are the same
          let config_text = environment.read_file("/dprint.json").unwrap();
          environment
            .write_file("/dprint.json", &config_text.replace(r#""non-existent": 1"#, r#""non-existent": 2"#))
            .unwrap();
          assert_eq!(client.not_formatted_message("/file.txt").await, expected);
          assert_eq!(client.take_messages(), shown);
        }
        .boxed_local()
      }
    });
    environment.take_stderr_messages();
  }

  #[test]
  fn should_cancel_a_format() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        // the plugin formats this until the format is cancelled
        let (message_id, receiver) = client.send_format("/file.txt", "wait_cancellation");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        client.notify(EditorMessageBody::CancelFormat(message_id));
        // the plugin stopped, so it can format another file
        assert_eq!(client.format("/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
        // there's no response for a cancelled format
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        client.pending.take(message_id);
        assert!(receiver.await.is_err());
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_format_a_range() {
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
        // a plugin that doesn't say whether it formats a range is given the range's
        // whole lines, which the test plugin puts its ending in place of, and only the
        // edits within those lines are kept, which leaves out the ending it adds to
        // the end of the text
        let options = FormatFileOptions {
          override_config: ConfigKeyMap::from([("ending".to_string(), "x".into())]),
          ..Default::default()
        };
        assert_eq!(
          client.format_range("/file.txt", "a\nb\nc\n", 2..3, Some(options)).await.unwrap(),
          formatted("a\n_x_c\n")
        );
        // a plugin that formats a range is given it as it is and what it changes is kept
        assert_eq!(
          client.format_range("/file.txt_ps", "a\nbc\nd", 3..4, None).await.unwrap(),
          formatted("a\nb_formatted_process_\nd_formatted_process")
        );
        // a notebook cell's range is formatted the same way
        let cell_options = FormatFileOptions {
          notebook_path: Some(PathBuf::from("/dir/notebook.ipynb")),
          override_config: ConfigKeyMap::from([("ending".to_string(), "x".into())]),
          ..Default::default()
        };
        assert_eq!(
          client.format_range("/dir/code_block.txt", "a\nb_x", 0..1, Some(cell_options)).await.unwrap(),
          formatted("_x_b_x")
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_say_why_a_notebook_cell_was_not_formatted() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_excludes("excluded");
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        let message = |reason, message: &str| NotFormattedMessage {
          reason,
          message: Some(message.to_string()),
          diagnostics: Vec::new(),
        };
        let cell_options = |notebook_path: &str| FormatFileOptions {
          notebook_path: Some(PathBuf::from(notebook_path)),
          ..Default::default()
        };
        // no plugin formats the notebook even though one formats the cell
        assert_eq!(
          client
            .format_edits("/dir/code_block.txt", "text", 0..4, Some(cell_options("/dir/notebook.ipynb")))
            .await
            .unwrap()
            .unwrap_err(),
          message(NotFormattedReason::NoPlugin, NO_CELL_PLUGIN_MESSAGE)
        );
        assert_eq!(
          client
            .format_edits("/excluded/code_block.txt", "text", 0..4, Some(cell_options("/excluded/notebook.ipynb")))
            .await
            .unwrap()
            .unwrap_err(),
          message(NotFormattedReason::NotMatched, NOT_MATCHED_MESSAGE)
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_format_untitled_documents_as_a_file_of_their_language() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/project/dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        client
          .update_settings(EditorSettings {
            workspace_folders: vec![uri("/project")],
            ..Default::default()
          })
          .await
          .unwrap();
        let options = |language| FormatFileOptions {
          language: Some(language),
          ..Default::default()
        };
        // the first file extension that the config file formats
        assert_eq!(
          client
            .format("untitled:Untitled-1", "text", Some(options(language("plaintext", &[".unknown", ".txt"]))))
            .await
            .unwrap(),
          formatted("text_formatted")
        );
        // the file extension dprint knows for the language
        assert_eq!(
          client.format("untitled:Untitled-1", "text", Some(options(language("txt", &[])))).await.unwrap(),
          formatted("text_formatted")
        );
        assert_eq!(
          client
            .format("untitled:Untitled-1", "text", Some(options(language("unknown-language", &[".unknown"]))))
            .await
            .unwrap(),
          not_formatted(NotFormattedReason::NoPlugin)
        );
        let file_info = client
          .file_info(FileInfoMessage {
            document: DocumentInfo {
              uri: uri("untitled:Untitled-1"),
              language: Some(language("plaintext", &[".txt"])),
              notebook_uri: None,
            },
            allow_global_config: false,
          })
          .await
          .unwrap();
        assert!(file_info.can_format);
        assert_eq!(file_info.config_file, Some(local_config("/project/dprint.json")));
        // the language is needed to name the file
        assert_eq!(
          client.format("untitled:Untitled-1", "text", None).await.unwrap_err().to_string(),
          "Provide the language of the document to format it: untitled:Untitled-1"
        );
        assert_eq!(
          client.format("https://dprint.dev/file.txt", "text", None).await.unwrap_err().to_string(),
          "Cannot format document that is not a file, an untitled document or a notebook cell: https://dprint.dev/file.txt"
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_send_the_progress_and_what_is_logged() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .build(); // not initialized, so the plugin is compiled when it's used

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(client.format("/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
        assert_eq!(
          client.take_progress(),
          vec![
            ProgressMessage::Begin {
              id: 0,
              title: "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
              percentage: Some(0),
            },
            ProgressMessage::End { id: 0 },
          ]
        );
        // debug messages are only sent when the setting is on
        assert!(client.take_logs().iter().all(|log| !log.starts_with("debug: ")));
        client
          .update_settings(EditorSettings {
            verbose: Some(true),
            ..Default::default()
          })
          .await
          .unwrap();
        client.format("/file.txt", "text", None).await.unwrap();
        let logs = client.take_logs();
        assert!(logs.iter().any(|log| log.starts_with("debug: ")), "{logs:?}");
        assert!(logs.iter().all(|log| !log.contains("[DEBUG]")), "{logs:?}");
      }
      .boxed_local()
    });
    // what's logged goes to the editor instead
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn should_send_the_diagnostics_of_config_files() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    let config_text = environment.read_file("/dprint.json").unwrap();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          // there's nothing to send when it has no problems
          assert_eq!(client.format("/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
          assert!(client.take_config_diagnostics().is_empty());

          // where it doesn't parse
          environment.write_file("/dprint.json", "{\n  \"a\": }").unwrap();
          assert!(client.format("/file.txt", "text", None).await.is_err());
          let diagnostics = client.take_config_diagnostics();
          assert_eq!(diagnostics.len(), 1);
          assert_eq!(diagnostics[0].uri, uri("/dprint.json"));
          assert_eq!(diagnostics[0].diagnostics.len(), 1);
          assert_eq!(diagnostics[0].diagnostics[0].range, Some(Range::new(Position::new(1, 7), Position::new(1, 8))));

          // the problems with the configuration of a plugin, which are sent once it's set up
          let config_text = config_text.replacen('{', "{\n  \"test-plugin\": { \"non-existent\": 1 },", 1);
          environment.write_file("/dprint.json", &config_text).unwrap();
          client.format("/file.txt", "text", None).await.unwrap();
          let diagnostics = client.wait_for_config_diagnostics(2).await;
          assert_eq!(
            diagnostics,
            vec![
              // the plugins weren't set up yet
              ConfigDiagnosticsMessage {
                uri: uri("/dprint.json"),
                diagnostics: Vec::new(),
              },
              ConfigDiagnosticsMessage {
                uri: uri("/dprint.json"),
                diagnostics: vec![ConfigDiagnostic {
                  message: "Unknown property in configuration".to_string(),
                  plugin_name: Some("test-plugin".to_string()),
                  property_name: Some("non-existent".to_string()),
                  range: Some(Range::new(Position::new(1, 19), Position::new(1, 33))),
                }],
              },
            ]
          );
          let logs = client.take_logs();
          assert!(
            logs.contains(&"warn: [test-plugin]: Unknown property in configuration (non-existent)".to_string()),
            "{logs:?}"
          );
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_say_when_a_config_file_changed_in_a_way_the_editor_cannot_see() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/base.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    environment.write_file("/a/dprint.json", r#"{ "extends": "./base.json" }"#).unwrap();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          assert_eq!(client.format("/a/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
          assert_eq!(client.format("/a/file.txt", "text", None).await.unwrap(), formatted("text_formatted"));
          assert!(client.take_config_changes().is_empty());

          // the config file that it extends changed
          let base_text = environment.read_file("/a/base.json").unwrap();
          environment
            .write_file("/a/base.json", &base_text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1))
            .unwrap();
          assert_eq!(client.format("/a/file.txt", "text", None).await.unwrap(), formatted("text_changed"));
          assert_eq!(client.take_config_changes(), vec![ConfigChangedMessage { uri: uri("/a/dprint.json") }]);
        }
        .boxed_local()
      }
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
      document: DocumentInfo {
        uri: uri(file_path),
        language: None,
        notebook_uri: None,
      },
      allow_global_config: false,
    }
  }

  fn format_file_body(file_path: &str, text: &str, range: Option<std::ops::Range<u32>>, options: FormatFileOptions) -> EditorMessageBody {
    let language = options.language.or_else(|| {
      // a notebook cell's language is the file extension of its path in the tests
      options.notebook_path.as_ref()?;
      let extension = Path::new(file_path).extension()?.to_string_lossy().to_string();
      Some(language(&extension, &[&format!(".{}", extension)]))
    });
    let message = FormatFileMessage {
      document: DocumentInfo {
        uri: uri(file_path),
        language,
        notebook_uri: options.notebook_path.map(|path| uri(&path.to_string_lossy().replace('\\', "/"))),
      },
      range: range.map(|range| OffsetRange {
        start: range.start,
        end: range.end,
      }),
      override_config: options.override_config,
      allow_global_config: options.allow_global_config,
      ensure_stable_format: options.ensure_stable_format,
      show_no_config_notification: options.show_no_config_notification,
    };
    EditorMessageBody::FormatFile(FormatFileMessageBody {
      data: serde_json::to_vec(&message).unwrap(),
      file_bytes: text.as_bytes().to_vec(),
    })
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
      uri: uri(path),
      is_global: false,
    }
  }

  /// Gets the uri of a document, which is a file uri for a path.
  fn uri(path_or_uri: &str) -> Uri {
    if path_or_uri.starts_with('/') {
      file_path_to_uri(Path::new(path_or_uri)).unwrap()
    } else {
      path_or_uri.parse().unwrap()
    }
  }

  fn language(id: &str, file_extensions: &[&str]) -> DocumentLanguage {
    DocumentLanguage {
      id: id.to_string(),
      file_extensions: file_extensions.iter().map(ToString::to_string).collect(),
      file_names: Vec::new(),
    }
  }

  /// The options of formatting a document in the tests.
  #[derive(Debug, Default, Clone)]
  struct FormatFileOptions {
    language: Option<DocumentLanguage>,
    /// The notebook when the document is one of its cells, whose language
    /// defaults to the file extension of the document's path.
    notebook_path: Option<PathBuf>,
    override_config: ConfigKeyMap,
    allow_global_config: bool,
    ensure_stable_format: Option<bool>,
    show_no_config_notification: Option<bool>,
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
    logs: Vec<LogMessage>,
    progress: Vec<ProgressMessage>,
    config_diagnostics: Vec<ConfigDiagnosticsMessage>,
    config_changes: Vec<ConfigChangedMessage>,
    /// The title of the action to choose for a message request.
    answer: Option<String>,
    /// Whether to respond to a message request with an error, which is what
    /// an editor does when it fails to show the message.
    answer_with_error: bool,
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
        while let Ok(message) = EditorMessage::read(&mut reader) {
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
            EditorMessageBody::Log(data) => {
              response_client.shown.borrow_mut().logs.push(serde_json::from_slice(data).unwrap());
              continue;
            }
            EditorMessageBody::Progress(data) => {
              response_client.shown.borrow_mut().progress.push(serde_json::from_slice(data).unwrap());
              continue;
            }
            EditorMessageBody::ConfigDiagnostics(data) => {
              response_client
                .shown
                .borrow_mut()
                .config_diagnostics
                .push(serde_json::from_slice(data).unwrap());
              continue;
            }
            EditorMessageBody::ConfigChanged(data) => {
              response_client.shown.borrow_mut().config_changes.push(serde_json::from_slice(data).unwrap());
              continue;
            }
            EditorMessageBody::ShowMessageRequest(data) => {
              let mut shown = response_client.shown.borrow_mut();
              shown.message_requests.push(serde_json::from_slice(data).unwrap());
              let body = if shown.answer_with_error {
                EditorMessageBody::Error(message.id, b"Failed showing the message.".to_vec())
              } else {
                let response = ShowMessageRequestResponseMessage {
                  action: shown.answer.clone().map(|title| MessageAction { title }),
                };
                EditorMessageBody::ShowMessageRequestResponse(message.id, serde_json::to_vec(&response).unwrap())
              };
              response_client.notify(body);
              continue;
            }
            EditorMessageBody::Success(message_id)
            | EditorMessageBody::Error(message_id, _)
            | EditorMessageBody::FileInfoResponse(message_id, _)
            | EditorMessageBody::ConfigInfoResponse(message_id, _)
            | EditorMessageBody::ConfigSchemaResponse(message_id, _)
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

    pub fn set_message_request_error(&self, value: bool) {
      self.shown.borrow_mut().answer_with_error = value;
    }

    pub fn take_messages(&self) -> Vec<ShowMessageMessage> {
      std::mem::take(&mut self.shown.borrow_mut().messages)
    }

    pub fn take_message_requests(&self) -> Vec<ShowMessageRequestMessage> {
      std::mem::take(&mut self.shown.borrow_mut().message_requests)
    }

    /// Takes the text of what was logged at each level.
    pub fn take_logs(&self) -> Vec<String> {
      std::mem::take(&mut self.shown.borrow_mut().logs)
        .into_iter()
        .map(|log| format!("{}: {}", serde_json::to_value(log.level).unwrap().as_str().unwrap(), log.message))
        .collect()
    }

    pub fn take_progress(&self) -> Vec<ProgressMessage> {
      std::mem::take(&mut self.shown.borrow_mut().progress)
    }

    pub fn take_config_diagnostics(&self) -> Vec<ConfigDiagnosticsMessage> {
      std::mem::take(&mut self.shown.borrow_mut().config_diagnostics)
    }

    /// Waits for the service to have sent the number of config diagnostics
    /// messages, which it may send after responding to a message.
    pub async fn wait_for_config_diagnostics(&self, count: usize) -> Vec<ConfigDiagnosticsMessage> {
      for _ in 0..100 {
        if self.shown.borrow().config_diagnostics.len() >= count {
          break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
      self.take_config_diagnostics()
    }

    pub fn take_config_changes(&self) -> Vec<ConfigChangedMessage> {
      std::mem::take(&mut self.shown.borrow_mut().config_changes)
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

    pub async fn config_schema(&self, file_paths: &[&str]) -> Result<serde_json::Value> {
      let message = ConfigSchemaMessage {
        uris: file_paths.iter().map(|file_path| uri(file_path)).collect(),
        allow_global_config: false,
      };
      match self.request(EditorMessageBody::ConfigSchema(serde_json::to_vec(&message)?)).await? {
        EditorMessageBody::ConfigSchemaResponse(_, data) => Ok(serde_json::from_slice::<ConfigSchemaResponseMessage>(&data)?.schema),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    /// Gets why the file wasn't formatted.
    pub async fn not_formatted_message(&self, file_path: &str) -> NotFormattedMessage {
      self.format_edits(file_path, "text", 0..4, None).await.unwrap().unwrap_err()
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
        Err(message) => FormatOutcome::NotFormatted(message.reason),
      })
    }

    /// Gets the edits that format the text or why it wasn't formatted.
    pub async fn format_edits(
      &self,
      file_path: impl AsRef<Path>,
      text: &str,
      range: std::ops::Range<u32>,
      options: Option<FormatFileOptions>,
    ) -> Result<Result<Vec<FormatEdit>, NotFormattedMessage>> {
      let file_path = file_path.as_ref().to_string_lossy().replace('\\', "/");
      let body = format_file_body(&file_path, text, Some(range), options.unwrap_or_default());
      match self.request(body).await? {
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::Formatted(data)) => Ok(Ok(serde_json::from_slice::<FormattedMessage>(&data)?.edits)),
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::NotFormatted(data)) => Ok(Err(serde_json::from_slice(&data)?)),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    /// Sends a format of the whole text without waiting for the response,
    /// providing the message id and the receiver of the response.
    pub fn send_format(&self, file_path: &str, text: &str) -> (u32, oneshot::Receiver<EditorMessageBody>) {
      let message_id = self.id_generator.next();
      let (tx, rx) = oneshot::channel();
      self.pending.store(message_id, tx);
      let body = format_file_body(file_path, text, None, Default::default());
      self.writer.send(EditorMessage { id: message_id, body }).unwrap();
      (message_id, rx)
    }

    /// Sends a message that doesn't get a response.
    pub fn notify(&self, body: EditorMessageBody) {
      self
        .writer
        .send(EditorMessage {
          id: self.id_generator.next(),
          body,
        })
        .unwrap();
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
