//! The editor service of schema version 5, which formats the files of one
//! config file that's found from the current directory.

use anyhow::Result;
use anyhow::anyhow;
use dprint_core::communication::RcIdStore;
use dprint_core::plugins::HostFormatRequest;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

mod messages;

use crate::arg_parser::CliArgs;
use crate::configuration::Permissions;
use crate::configuration::ResolvedConfig;
use crate::configuration::SessionPermissions;
use crate::configuration::resolve_config_from_args;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::plugins::PluginResolver;
use crate::resolution::PluginsScope;
use crate::resolution::UnapprovedProcessPlugins;
use crate::resolution::get_cant_ask_message;
use crate::resolution::get_plugins_scope_from_args_with_approval;
use crate::resolution::resolve_plugins_scope_with_approval;
use crate::utils::Semaphore;

use super::CONFIG_SCHEMA_URL;
use super::EditorContext;
use super::new_format_concurrency_limiter;
use super::send_error_response;
use super::send_response_body;
use super::start_reading_messages;

use self::messages::EditorMessageBody;
use self::messages::EditorPluginInfo;
use self::messages::FormatEditorMessageBody;

/// Outputs the information about the plugins of the config file for an editor
/// using schema version 5 of the editor service (`dprint editor-info`).
pub async fn output_editor_info<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorInfo {
    schema_version: u32,
    cli_version: String,
    config_schema_url: String,
    plugins: Vec<EditorPluginInfo>,
  }

  // IMPORTANT: An editor gets this when it opens a project, which isn't the same
  // as the user choosing to run dprint there, so only provide the plugins when
  // the user allowed its process plugins.
  let permissions = SessionPermissions::default().read(environment);
  let scope = get_plugins_scope_from_args_with_approval(args, environment, plugin_resolver, &permissions).await?;
  scope.ensure_no_global_config_diagnostics()?;
  if let Some(unapproved) = &scope.unapproved_process_plugins {
    log_warn!(
      environment,
      "{} {}",
      unapproved.message(&permissions),
      get_cant_ask_message(environment, &unapproved.config_file)
    );
  }
  let plugins = get_editor_plugin_infos(&scope).await?;

  environment.log_machine_readable(&serde_json::to_vec(&EditorInfo {
    // IMPORTANT: Don't change this. Older editor extensions error for a higher
    // schema version, so the newer ones are found with `editor-service --info`.
    schema_version: 5,
    cli_version: environment.cli_version(),
    config_schema_url: CONFIG_SCHEMA_URL.to_string(),
    plugins,
  })?);

  Ok(())
}

pub async fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let mut editor_service = EditorService::new(args, environment, plugin_resolver);
  editor_service.run().await
}

/// A message that needs the configuration to be handled.
enum ConfigMessage {
  CanFormat(u32, PathBuf),
  Format(u32, FormatEditorMessageBody, Arc<CancellationToken>),
}

struct EditorService<'a, TEnvironment: Environment> {
  args: &'a CliArgs,
  environment: &'a TEnvironment,
  plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>,
  plugins_scope: Option<Rc<PluginsScope<TEnvironment>>>,
  /// What the user allowed when the plugins were resolved.
  permissions: Option<Permissions>,
  session_permissions: SessionPermissions,
  /// The process plugins that weren't allowed to run that the log said so about.
  warned_process_plugins: HashSet<UnapprovedProcessPlugins>,
  context: Rc<EditorContext<EditorMessageBody>>,
  /// Tokens of the format requests that are waiting to be handled.
  pending_format_tokens: RcIdStore<Arc<CancellationToken>>,
  concurrency_limiter: Rc<Semaphore>,
  config_semaphore: Rc<Semaphore>,
  ensure_stable_format: EnsureStableFormat,
}

impl<'a, TEnvironment: Environment> EditorService<'a, TEnvironment> {
  pub fn new(args: &'a CliArgs, environment: &'a TEnvironment, plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>) -> Self {
    Self {
      args,
      environment,
      plugin_resolver,
      plugins_scope: None,
      permissions: None,
      session_permissions: Default::default(),
      warned_process_plugins: Default::default(),
      context: Rc::new(EditorContext::new(environment)),
      pending_format_tokens: Default::default(),
      concurrency_limiter: Rc::new(new_format_concurrency_limiter(environment)),
      config_semaphore: Rc::new(Semaphore::new(1)),
      ensure_stable_format: EnsureStableFormat::for_editor(environment),
    }
  }

  pub async fn run(&mut self) -> Result<()> {
    let mut rx = start_reading_messages::<EditorMessageBody>(self.environment);
    // The messages that need the configuration are handled one at a time on a
    // separate task from the one receiving the messages. Resolving the
    // configuration can take a long time (ex. downloading plugins), so this
    // keeps the service responding to the other messages while that happens.
    let (config_message_tx, mut config_message_rx) = tokio::sync::mpsc::unbounded_channel::<ConfigMessage>();
    let context = self.context.clone();
    let pending_format_tokens = self.pending_format_tokens.clone();
    let receive_messages = async move {
      loop {
        let message = tokio::select! {
          message = rx.recv() => message,
          // there's nobody to respond to anymore
          _ = context.output_closed.cancelled() => None,
        };
        let Some(message) = message else { return };
        match message.body {
          EditorMessageBody::Success(_message_id) => {}
          EditorMessageBody::Error(_message_id, _data) => {}
          EditorMessageBody::Close => {
            handle_message(&context, message.id, || Ok(EditorMessageBody::Success(message.id)));
            // the process exits once this returns
            context.writer.flush().await;
            return;
          }
          EditorMessageBody::IsAlive => {
            handle_message(&context, message.id, || Ok(EditorMessageBody::Success(message.id)));
          }
          EditorMessageBody::CanFormat(path) => {
            let _ = config_message_tx.send(ConfigMessage::CanFormat(message.id, path));
          }
          EditorMessageBody::CanFormatResponse(_, _) => {
            send_error_response(&context, message.id, anyhow!("CLI cannot handle a CanFormatResponse message."));
          }
          EditorMessageBody::Format(body) => {
            // store this now so the format can be cancelled while it's waiting to be handled
            let token = Arc::new(CancellationToken::new());
            pending_format_tokens.store(message.id, token.clone());
            let _ = config_message_tx.send(ConfigMessage::Format(message.id, body, token));
          }
          EditorMessageBody::FormatResponse(_, _) => {
            send_error_response(&context, message.id, anyhow!("CLI cannot handle a FormatResponse message."));
          }
          EditorMessageBody::CancelFormat(message_id) => {
            if let Some(token) = context.cancellation_tokens.take(message_id) {
              token.cancel();
            } else if let Some(token) = pending_format_tokens.get_cloned(message_id) {
              token.cancel();
            }
          }
          EditorMessageBody::Unknown(message_kind, _) => {
            send_error_response(&context, message.id, anyhow!("Unknown message with kind: {}", message_kind));
          }
        }
      }
    };
    let handle_config_messages = async {
      while let Some(message) = config_message_rx.recv().await {
        match message {
          ConfigMessage::CanFormat(message_id, path) => {
            let result = self.can_format(&path).await;
            handle_message(&self.context, message_id, || {
              result.map(|val| EditorMessageBody::CanFormatResponse(message_id, if val { 1 } else { 0 }))
            });
          }
          ConfigMessage::Format(message_id, body, token) => {
            self.pending_format_tokens.take(message_id);
            if !token.is_cancelled() {
              self.format(message_id, body, token).await;
            }
          }
        }
      }
    };

    tokio::select! {
      // finished when the editor closes the service or its output
      _ = receive_messages => {}
      _ = handle_config_messages => {}
    }
    Ok(())
  }

  async fn format(&mut self, message_id: u32, body: FormatEditorMessageBody, token: Arc<CancellationToken>) {
    if self.plugins_scope.is_none()
      && let Err(err) = self.ensure_latest_config().await
    {
      // respond with the error instead of exiting so the editor can
      // show it and the config file can be fixed without a restart
      send_error_response(&self.context, message_id, err);
      return;
    }
    let request = HostFormatRequest {
      // canonicalize so that it matches the canonicalized config paths (ex. for overrides)
      file_path: self
        .environment
        .canonicalize_maybe_not_exists(&body.file_path)
        .map(|p| p.into_path_buf())
        .unwrap_or(body.file_path),
      range: body.range,
      override_config: if body.override_config.is_empty() {
        Default::default()
      } else {
        match serde_json::from_slice(&body.override_config) {
          Ok(config) => config,
          Err(err) => {
            send_error_response(&self.context, message_id, anyhow!("Error deserializing override config. {:#}", err));
            return;
          }
        }
      },
      file_bytes: body.file_bytes,
      token: token.clone(),
    };

    let token_storage_guard = self.context.cancellation_tokens.store_with_owned_guard(message_id, token.clone());
    let context = self.context.clone();
    let concurrency_limiter = self.concurrency_limiter.clone();
    let scope = self.plugins_scope.clone().unwrap();
    let ensure_stable_format = self.ensure_stable_format;
    let _ignore = dprint_core::async_runtime::spawn(async move {
      let _permit = concurrency_limiter.acquire().await;
      if token.is_cancelled() {
        return;
      }

      let result = scope.format_stable(request, ensure_stable_format).await;
      drop(token_storage_guard);
      if token.is_cancelled() {
        return;
      }

      let body = match result {
        Ok(text) => EditorMessageBody::FormatResponse(message_id, text),
        Err(err) => EditorMessageBody::Error(message_id, dprint_core::plugins::error_to_string(&err).into_bytes()),
      };
      send_response_body(&context, body);
    });
  }

  async fn can_format(&mut self, file_path: &Path) -> Result<bool> {
    self.ensure_latest_config().await?;
    // canonicalize the file path, then check if it's in the list of file paths.
    let file_path = self
      .environment
      .canonicalize_maybe_not_exists(file_path)
      .map(|p| p.into_path_buf())
      .unwrap_or(file_path.to_path_buf());
    log_debug!(self.environment, "Checking can format: {}", file_path.display());
    // the editor service only receives a path, so an extensionless file's
    // shebang is resolved from the file on disk
    Ok(self.plugins_scope.as_ref().map(|s| s.can_format_for_editor(&file_path, None)).unwrap_or(false))
  }

  async fn ensure_latest_config(&mut self) -> Result<Rc<ResolvedConfig>> {
    let _update_permit = self.config_semaphore.acquire().await;
    let config = Rc::new(resolve_config_from_args(self.args, self.environment).await?);
    // read each time so allowing process plugins or editing the file takes effect
    let permissions = self.session_permissions.read(self.environment);

    let last_config = self.plugins_scope.as_ref().and_then(|scope| scope.config.as_ref());
    let has_config_changed =
      last_config.is_none() || last_config.unwrap() != &config || self.plugins_scope.is_none() || self.permissions.as_ref() != Some(&permissions);
    if has_config_changed {
      self.plugins_scope.take();
      let mut tokens = self.context.cancellation_tokens.take_all().into_iter().collect::<Vec<_>>();
      tokens.sort_by_key(|(message_id, _)| *message_id);
      for (message_id, token) in tokens {
        token.cancel();
        // the editor didn't ask for these to be cancelled, so respond in
        // order to not leave it waiting on them
        send_error_response(
          &self.context,
          message_id,
          anyhow!("Formatting was cancelled because the configuration changed."),
        );
      }
      self.plugin_resolver.clear_and_shutdown_initialized().await;

      // IMPORTANT: Process plugins don't run sandboxed, so only use the plugins
      // when the user allowed its process plugins. The editor starting the service
      // in a project (ex. one that was just cloned) isn't the same as the user
      // choosing to run dprint there.
      let scope = resolve_plugins_scope_with_approval(config.clone(), self.environment, self.plugin_resolver, &permissions).await?;
      scope.ensure_no_global_config_diagnostics()?;
      if let Some(unapproved) = &scope.unapproved_process_plugins {
        self.warn_unapproved_process_plugins(unapproved, &permissions);
      }
      self.plugins_scope = Some(Rc::new(scope));
      self.permissions = Some(permissions);
    }

    Ok(self.plugins_scope.as_ref().unwrap().config.clone().unwrap())
  }

  /// Says in the log that dprint isn't formatting with the config file because
  /// its process plugins weren't allowed to run, which is only done once per session for each
  /// configuration and not when the user denied them. Schema version 5 of
  /// the editor service can't ask the user whether to allow them.
  fn warn_unapproved_process_plugins(&mut self, unapproved: &UnapprovedProcessPlugins, permissions: &Permissions) {
    if !self.warned_process_plugins.insert(unapproved.clone()) || unapproved.is_denied(permissions) {
      return;
    }
    log_warn!(
      self.environment,
      "{} {}",
      unapproved.message(permissions),
      get_cant_ask_message(self.environment, &unapproved.config_file)
    );
  }
}

/// Gets the information about the plugins of the scope that an editor uses.
async fn get_editor_plugin_infos<TEnvironment: Environment>(scope: &PluginsScope<TEnvironment>) -> Result<Vec<EditorPluginInfo>> {
  let mut plugins = Vec::new();
  for plugin in scope.plugins.values() {
    let initialized_plugin = plugin.initialize().await?;
    let file_matching = initialized_plugin.file_matching_info().await?;
    plugins.push(EditorPluginInfo {
      name: plugin.info().name.to_string(),
      version: plugin.info().version.to_string(),
      config_key: plugin.info().config_key.to_string(),
      file_extensions: file_matching.file_extensions,
      file_names: file_matching.file_names,
      config_schema_url: if plugin.info().config_schema_url.trim().is_empty() {
        None
      } else {
        Some(plugin.info().config_schema_url.trim().to_string())
      },
      help_url: plugin.info().help_url.trim().to_string(),
      update_url: plugin.info().update_url.clone(),
    });
  }
  Ok(plugins)
}

fn handle_message(context: &EditorContext<EditorMessageBody>, original_message_id: u32, action: impl FnOnce() -> Result<EditorMessageBody>) {
  match action() {
    Ok(body) => send_response_body(context, body),
    Err(err) => send_error_response(context, original_message_id, err),
  };
}

#[cfg(test)]
mod test {
  use anyhow::Result;
  use anyhow::anyhow;
  use dprint_core::async_runtime::DropGuardAction;
  use dprint_core::async_runtime::FutureExt;
  use dprint_core::async_runtime::LocalBoxFuture;
  use dprint_core::async_runtime::future;
  use dprint_core::communication::IdGenerator;
  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::RcIdStore;
  use dprint_core::communication::SingleThreadMessageWriter;
  use dprint_core::configuration::ConfigKeyMap;
  use dprint_core::plugins::FormatError;
  use dprint_core::plugins::FormatRange;
  use dprint_core::plugins::FormatResult;
  use pretty_assertions::assert_eq;
  use std::io::Read;
  use std::io::Write;
  use std::path::Path;
  use std::path::PathBuf;
  use std::rc::Rc;
  use std::sync::Arc;
  use std::time::Duration;
  use tokio::sync::oneshot;
  use tokio_util::sync::CancellationToken;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;

  use super::super::test::disallow_process_plugins;

  use super::messages::EditorMessage;
  use super::messages::EditorMessageBody;
  use super::messages::FormatEditorMessageBody;

  #[test]
  fn should_output_editor_plugin_info() {
    // it should not output anything when downloading plugins
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .build(); // build only, don't initialize
    crate::test_helpers::allow_process_plugins_in_editors(&environment);
    run_test_cli(vec!["editor-info"], &environment).unwrap();
    let mut final_output = r#"{"schemaVersion":5,"cliVersion":""#.to_string();
    final_output.push_str(&environment.cli_version());
    final_output.push_str(r#"","configSchemaUrl":"https://dprint.dev/schemas/v0.json","plugins":["#);
    final_output
      .push_str(r#"{"name":"test-plugin","version":"0.2.0","configKey":"test-plugin","fileExtensions":["txt"],"fileNames":[],"configSchemaUrl":"https://plugins.dprint.dev/test/schema.json","helpUrl":"https://dprint.dev/plugins/test","updateUrl":"https://plugins.dprint.dev/dprint/test-plugin/latest.json"},"#);
    final_output.push_str(r#"{"name":"test-process-plugin","version":"0.1.0","configKey":"testProcessPlugin","fileExtensions":["txt_ps"],"fileNames":["test-process-plugin-exact-file"],"helpUrl":"https://dprint.dev/plugins/test-process","updateUrl":"https://plugins.dprint.dev/dprint/test-process-plugin/latest.json"}]}"#);
    assert_eq!(environment.take_stdout_messages(), vec![final_output]);
    let mut stderr_messages = environment.take_stderr_messages();
    stderr_messages.sort();
    assert_eq!(
      stderr_messages,
      vec![
        "Compiling https://plugins.dprint.dev/test-plugin.wasm",
        "Extracting zip for test-process-plugin"
      ]
    );
  }

  #[test]
  fn should_output_editor_plugin_info_with_global_config() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .build();
    run_test_cli(vec!["editor-info", "--config-discovery=global"], &environment).unwrap();
    let mut final_output = r#"{"schemaVersion":5,"cliVersion":""#.to_string();
    final_output.push_str(&environment.cli_version());
    final_output.push_str(r#"","configSchemaUrl":"https://dprint.dev/schemas/v0.json","plugins":["#);
    final_output
      .push_str(r#"{"name":"test-plugin","version":"0.2.0","configKey":"test-plugin","fileExtensions":["txt"],"fileNames":[],"configSchemaUrl":"https://plugins.dprint.dev/test/schema.json","helpUrl":"https://dprint.dev/plugins/test","updateUrl":"https://plugins.dprint.dev/dprint/test-plugin/latest.json"},"#);
    final_output.push_str(r#"{"name":"test-process-plugin","version":"0.1.0","configKey":"testProcessPlugin","fileExtensions":["txt_ps"],"fileNames":["test-process-plugin-exact-file"],"helpUrl":"https://dprint.dev/plugins/test-process","updateUrl":"https://plugins.dprint.dev/dprint/test-process-plugin/latest.json"}]}"#);
    assert_eq!(environment.take_stdout_messages(), vec![final_output]);
    let mut stderr_messages = environment.take_stderr_messages();
    stderr_messages.sort();
    assert_eq!(
      stderr_messages,
      vec![
        "Compiling https://plugins.dprint.dev/test-plugin.wasm",
        "Extracting zip for test-process-plugin"
      ]
    );
  }

  /// Runs the editor service while the action communicates with it.
  fn run_editor_service_session(environment: &TestEnvironment, action: impl FnOnce(EditorServiceCommunicator) -> LocalBoxFuture<'static, ()> + Send + 'static) {
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();
    let result = std::thread::spawn(move || {
      TestEnvironment::new().run_in_runtime(async move {
        let communicator = EditorServiceCommunicator::new(stdin, stdout);
        action(communicator.clone()).await;
        communicator.exit().await.unwrap();
      });
    });
    // usually this would be the editor's process id, but this is ok for testing purposes
    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], environment).unwrap();
    result.join().unwrap();
  }

  enum MessageResponseChannel {
    Success(oneshot::Sender<Result<()>>),
    Format(oneshot::Sender<Result<Option<Vec<u8>>>>),
    CanFormat(oneshot::Sender<Result<bool>>),
  }

  #[derive(Clone)]
  struct EditorServiceCommunicator {
    writer: Rc<SingleThreadMessageWriter<EditorMessage>>,
    id_generator: Rc<IdGenerator>,
    messages: RcIdStore<MessageResponseChannel>,
  }

  impl EditorServiceCommunicator {
    pub fn new(stdin: Box<dyn Write + Send>, stdout: Box<dyn Read + Send>) -> Self {
      let mut reader = MessageReader::new(stdout);
      let writer = Rc::new(SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdin)));

      let communicator = EditorServiceCommunicator {
        writer,
        id_generator: Default::default(),
        messages: Default::default(),
      };

      let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
      dprint_core::async_runtime::spawn_blocking({
        move || loop {
          let message = EditorMessage::read(&mut reader);
          let msg_was_err = message.is_err();
          if tx.send(message).is_err() || msg_was_err {
            break;
          }
        }
      });

      let messages = communicator.messages.clone();
      dprint_core::async_runtime::spawn(async move {
        while let Some(Ok(message)) = rx.recv().await {
          if let Err(_) = handle_stdout_message(message, &messages) {
            break;
          }
        }
      });

      communicator
    }

    pub async fn is_alive(&self) -> Result<()> {
      let (tx, rx) = oneshot::channel::<Result<()>>();

      self
        .send_message(
          EditorMessageBody::IsAlive,
          MessageResponseChannel::Success(tx),
          rx,
          Arc::new(CancellationToken::new()),
        )
        .await
    }

    pub async fn check_file(&self, file_path: impl AsRef<Path>) -> Result<bool> {
      let (tx, rx) = oneshot::channel::<Result<bool>>();

      self
        .send_message(
          EditorMessageBody::CanFormat(file_path.as_ref().to_path_buf()),
          MessageResponseChannel::CanFormat(tx),
          rx,
          Arc::new(CancellationToken::new()),
        )
        .await
    }

    pub async fn format_text(
      &self,
      file_path: impl AsRef<Path>,
      file_bytes: Vec<u8>,
      range: FormatRange,
      override_config: ConfigKeyMap,
      token: CancellationToken,
    ) -> FormatResult {
      let (tx, rx) = oneshot::channel::<Result<Option<Vec<u8>>>>();

      self
        .send_message(
          EditorMessageBody::Format(FormatEditorMessageBody {
            file_path: file_path.as_ref().to_path_buf(),
            file_bytes,
            override_config: serde_json::to_vec(&override_config).unwrap(),
            range,
          }),
          MessageResponseChannel::Format(tx),
          rx,
          Arc::new(token),
        )
        .await
        .map_err(FormatError::new)
    }

    pub async fn exit(&self) -> Result<()> {
      let (tx, rx) = oneshot::channel::<Result<()>>();

      self
        .send_message(
          EditorMessageBody::Close,
          MessageResponseChannel::Success(tx),
          rx,
          Arc::new(CancellationToken::new()),
        )
        .await
    }

    async fn send_message<T: Default>(
      &self,
      body: EditorMessageBody,
      response_channel: MessageResponseChannel,
      receiver: oneshot::Receiver<Result<T>>,
      token: Arc<CancellationToken>,
    ) -> Result<T> {
      let message_id = self.id_generator.next();
      let mut drop_guard = DropGuardAction::new(|| {
        let _ = self.writer.send(EditorMessage {
          id: self.id_generator.next(),
          body: EditorMessageBody::CancelFormat(message_id),
        });
        self.messages.take(message_id); // clear memory
      });
      self.messages.store(message_id, response_channel);
      self.writer.send(EditorMessage { id: message_id, body })?;
      tokio::select! {
        _ = token.cancelled() => {
          drop(drop_guard); // be explicit
          Ok(Default::default())
        }
        response = receiver => {
          drop_guard.forget(); // we completed successfully, so don't run the drop guard code
          match response {
            Ok(data) => data,
            Err(err) => panic!("{:#}", err)
          }
        }
      }
    }
  }

  fn handle_stdout_message(message: EditorMessage, messages: &RcIdStore<MessageResponseChannel>) -> Result<()> {
    match message.body {
      EditorMessageBody::Success(message_id) => match messages.take(message_id) {
        Some(MessageResponseChannel::Success(channel)) => {
          let _ignore = channel.send(Ok(()));
        }
        Some(_) => unreachable!(),
        None => {}
      },
      EditorMessageBody::Error(message_id, data) => {
        let err = anyhow!("{}", String::from_utf8_lossy(&data));
        match messages.take(message_id) {
          Some(MessageResponseChannel::Success(channel)) => {
            let _ignore = channel.send(Err(err));
          }
          Some(MessageResponseChannel::Format(channel)) => {
            let _ignore = channel.send(Err(err));
          }
          Some(MessageResponseChannel::CanFormat(channel)) => {
            let _ignore = channel.send(Err(err));
          }
          None => {}
        }
      }
      EditorMessageBody::FormatResponse(message_id, data) => match messages.take(message_id) {
        Some(MessageResponseChannel::Format(channel)) => {
          let _ignore = channel.send(Ok(data));
        }
        Some(_) => unreachable!(),
        None => {}
      },
      EditorMessageBody::CanFormatResponse(message_id, value) => match messages.take(message_id) {
        Some(MessageResponseChannel::CanFormat(channel)) => {
          let _ignore = channel.send(Ok(if value == 1 { true } else { false }));
        }
        Some(_) => unreachable!(),
        None => {}
      },
      _ => unreachable!(),
    }

    Ok(())
  }

  #[test]
  fn should_not_output_plugins_with_unapproved_process_plugins_in_editor_info() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .build(); // build only, don't initialize
    disallow_process_plugins(&environment);
    run_test_cli(vec!["editor-info"], &environment).unwrap();
    let stdout_messages = environment.take_stdout_messages();
    assert_eq!(stdout_messages.len(), 1);
    // the wasm plugin isn't used either so that it doesn't format differently than the cli
    assert!(stdout_messages[0].contains(r#""plugins":[]"#), "{stdout_messages:?}");
    // the process plugin is never set up, which runs it
    let stderr_messages = environment.take_stderr_messages();
    assert_eq!(stderr_messages.len(), 1, "{stderr_messages:?}");
    assert!(
      stderr_messages[0].starts_with("dprint isn't formatting files with /dprint.json because its process plugins ("),
      "{stderr_messages:?}"
    );
  }

  #[test]
  fn should_not_set_up_unapproved_process_plugins_for_editor_service() {
    let file_path = PathBuf::from("/file.txt_ps");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_process_plugin();
      })
      .write_file(&file_path, "text")
      .build(); // not initialized, so the process plugin is only set up when it's allowed
    disallow_process_plugins(&environment);

    run_editor_service_session(&environment, move |communicator| {
      async move {
        communicator.check_file(&file_path).await.unwrap();
        assert_eq!(
          communicator
            .format_text(&file_path, b"text".to_vec(), None, Default::default(), Default::default())
            .await
            .unwrap(),
          None
        );
      }
      .boxed_local()
    });

    let stderr_messages = environment.take_stderr_messages();
    assert_eq!(stderr_messages.len(), 1, "{stderr_messages:?}");
    assert!(
      stderr_messages[0].starts_with(concat!(
        "dprint isn't formatting files with /dprint.json because its process plugins ",
        "(https://plugins.dprint.dev/test-process.json@"
      )),
      "{stderr_messages:?}"
    );
    assert!(
      stderr_messages[0].contains(concat!(
        "dprint only formats with it once you allow them by running `dprint config install --config \"/dprint.json\"` ",
        "or when \"editor.allowProcessPlugins\" isn't false in"
      )),
      "{stderr_messages:?}"
    );
  }

  #[test]
  fn should_use_process_plugins_installed_with_cli_for_editor_service() {
    let file_path = PathBuf::from("/file.txt_ps");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_process_plugin();
      })
      .write_file(&file_path, "text")
      .build(); // not initialized, so the process plugin is only set up when it's allowed
    disallow_process_plugins(&environment);

    run_test_cli(vec!["config", "install"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["Installed test-process-plugin 0.1.0"]);

    run_editor_service_session(&environment, move |communicator| {
      async move {
        communicator.check_file(&file_path).await.unwrap();
        assert!(
          communicator
            .format_text(&file_path, b"text".to_vec(), None, Default::default(), Default::default())
            .await
            .unwrap()
            .is_some()
        );
      }
      .boxed_local()
    });
    let stderr_messages = environment.take_stderr_messages();
    assert!(!stderr_messages.iter().any(|message| message.contains("didn't use")), "{stderr_messages:?}");
  }

  #[test]
  fn should_format_for_editor_service() {
    let txt_file_path = PathBuf::from("/file.txt");
    let ts_file_path = PathBuf::from("/file.ts");
    let other_ext_path = PathBuf::from("/file.asdf");
    let ignored_file_path = PathBuf::from("/ignored_file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_includes("**/*.{txt,ts}")
          .add_excludes("ignored_file.txt")
          .add_excludes("ignored-dir");
      })
      .write_file(&txt_file_path, "")
      .write_file(&ts_file_path, "")
      .write_file(&other_ext_path, "")
      .write_file(&ignored_file_path, "text")
      .initialize()
      .build();
    crate::test_helpers::allow_process_plugins_in_editors(&environment);
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      let environment = environment.clone();
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
          assert_eq!(communicator.check_file(&PathBuf::from("/non-existent.txt")).await.unwrap(), true);
          assert_eq!(communicator.check_file(&PathBuf::from("/ignored-dir/some-path.txt")).await.unwrap(), false);
          assert_eq!(communicator.check_file(&PathBuf::from("/ignored-dir/sub/some-path.txt")).await.unwrap(), false);
          assert_eq!(communicator.check_file(&other_ext_path).await.unwrap(), false);
          assert_eq!(communicator.check_file(&ts_file_path).await.unwrap(), true);
          assert_eq!(communicator.check_file(&ignored_file_path).await.unwrap(), false);

          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_formatted"
          );
          assert_eq!(
            communicator
              .format_text(
                &txt_file_path,
                "testing_formatted".to_string().into_bytes(),
                None,
                Default::default(),
                Default::default()
              )
              .await
              .unwrap()
              .is_none(),
            true
          ); // it is already formatted
          assert_eq!(
            communicator
              .format_text(
                &other_ext_path,
                "testing".to_string().into_bytes(),
                None,
                Default::default(),
                Default::default()
              )
              .await
              .unwrap()
              .is_none(),
            true
          ); // can't format
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &txt_file_path,
                  "plugin: format this text".to_string().into_bytes(),
                  None,
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "plugin: format this text_formatted_process_formatted"
          );
          assert_eq!(
            communicator
              .format_text(
                &txt_file_path,
                "should_error".to_string().into_bytes(),
                None,
                Default::default(),
                Default::default()
              )
              .await
              .err()
              .unwrap()
              .to_string(),
            "Did error."
          );
          assert_eq!(
            communicator
              .format_text(
                &txt_file_path,
                "plugin: should_error".to_string().into_bytes(),
                None,
                Default::default(),
                Default::default()
              )
              .await
              .err()
              .unwrap()
              .to_string(),
            "Did error."
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt_ps"),
                  "testing".to_string().into_bytes(),
                  None,
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "testing_formatted_process"
          );

          // try parallelizing many things
          let mut handles = Vec::new();
          for _ in 0..50 {
            handles.push(dprint_core::async_runtime::spawn({
              let communicator = communicator.clone();
              let txt_file_path = txt_file_path.clone();
              async move {
                assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
              }
            }));
            handles.push(dprint_core::async_runtime::spawn({
              let communicator = communicator.clone();
              let txt_file_path = txt_file_path.clone();
              async move {
                assert_eq!(
                  bytes_to_string(
                    communicator
                      .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
                      .await
                      .unwrap()
                      .unwrap()
                  ),
                  "testing_formatted"
                );
              }
            }));
            handles.push(dprint_core::async_runtime::spawn({
              let communicator = communicator.clone();
              async move {
                assert_eq!(
                  bytes_to_string(
                    communicator
                      .format_text(
                        &PathBuf::from("/file.txt_ps"),
                        "testing".to_string().into_bytes(),
                        None,
                        Default::default(),
                        Default::default()
                      )
                      .await
                      .unwrap()
                      .unwrap()
                  ),
                  "testing_formatted_process"
                );
              }
            }));
          }

          // ensure nothing panicked
          let results = future::join_all(handles).await;
          for result in results {
            result.unwrap();
          }

          // test process range formatting
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt_ps"),
                  "testing".to_string().into_bytes(),
                  Some(1..2),
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "t_formatted_process_sting_formatted_process"
          );
          // test process plugin host formatting with range
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt_ps"),
                  "plugin-range: testing".to_string().into_bytes(),
                  Some(1..2),
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "plugin-range: t_formatted_sting_formatted_formatted_process"
          );

          // test wasm range formatting
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt"),
                  "testing".to_string().into_bytes(),
                  Some(1..2),
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "t_formatted_sting_formatted"
          );

          // test wasm plugin host formatting with range
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt"),
                  "plugin-range: testing".to_string().into_bytes(),
                  Some(1..2),
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "plugin-range: t_formatted_process_sting_formatted_process_formatted"
          );

          // test process and wasm plugin cancellation
          for file_name in ["/file.txt_ps", "/file.txt"] {
            let token = CancellationToken::new();
            let handle = dprint_core::async_runtime::spawn({
              let communicator = communicator.clone();
              let token = token.clone();
              async move {
                assert_eq!(
                  communicator
                    .format_text(
                      &PathBuf::from(file_name),
                      "wait_cancellation".to_string().into_bytes(),
                      None,
                      Default::default(),
                      token
                    )
                    .await
                    .unwrap(),
                  None
                )
              }
            });

            // give some time for the message to be sent
            tokio::time::sleep(Duration::from_millis(50)).await;
            token.cancel();
            handle.await.unwrap();
          }

          // test override config
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &PathBuf::from("/file.txt_ps"),
                  "testing".to_string().into_bytes(),
                  Some(2..5),
                  {
                    let mut config = ConfigKeyMap::new();
                    config.insert("ending".to_string(), "test".into());
                    config
                  },
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "te_test_ng_test"
          );

          // write a new file and make sure the service picks up the changes
          environment
            .write_file(
              &PathBuf::from("./dprint.json"),
              r#"{
                    "includes": ["**/*.txt"],
                    "test-plugin": {
                        "ending": "new_ending"
                    },
                    "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
                }"#,
            )
            .unwrap();

          assert_eq!(communicator.check_file(&ts_file_path).await.unwrap(), false); // shouldn't match anymore
          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true); // still ok
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "testing_new_ending"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    // usually this would be the editor's process id, but this is ok for testing purposes
    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_inherit_ancestor_config_for_editor_service() {
    let txt_file_path = PathBuf::from("/sub_dir/file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "root" }"#);
      })
      .with_local_config("/sub_dir/dprint.json", |c| {
        c.set_inherit(true);
      })
      .write_file(&txt_file_path, "")
      .initialize()
      .set_cwd("/sub_dir")
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_root"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_warn_for_inherit_without_ancestor_config_for_editor_service() {
    let txt_file_path = PathBuf::from("/project/file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      // the global config file is not inherited from
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "global" }"#);
      })
      .with_local_config("/project/dprint.json", |c| {
        c.set_inherit(true).add_remote_wasm_plugin();
      })
      .write_file(&txt_file_path, "")
      .set_cwd("/project")
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          // the config is resolved for each of these
          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_formatted"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();

    // only warns once
    let logs = environment.take_stderr_messages();
    let logs = logs.iter().filter(|log| !log.starts_with("Compiling ")).collect::<Vec<_>>();
    assert_eq!(logs.len(), 1, "{:?}", logs);
    assert!(
      logs[0].ends_with(concat!(
        "The 'inherit' property is ignored in /project/dprint.json because there is no ",
        "configuration file in an ancestor directory to inherit from."
      )),
      "{}",
      logs[0]
    );
  }

  #[test]
  fn should_respond_with_error_for_format_with_config_error_for_editor_service() {
    let txt_file_path = PathBuf::from("/file.txt");
    let config_file_path = PathBuf::from("/dprint.json");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&txt_file_path, "")
      .initialize()
      .build();
    let config_file_text = environment.read_file(&config_file_path).unwrap();
    environment.write_file(&config_file_path, "{ invalid").unwrap();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      let environment = environment.clone();
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);
          let format = || communicator.format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default());

          // a format request is the first message some editors send
          let err = format().await.err().unwrap();
          assert!(err.to_string().contains("dprint.json"), "{}", err);
          let err = format().await.err().unwrap();
          assert!(err.to_string().contains("dprint.json"), "{}", err);

          // keeps running, so formatting works once the config file is fixed
          environment.write_file(&config_file_path, &config_file_text).unwrap();
          assert_eq!(format().await.unwrap().unwrap(), b"testing_formatted");

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_clamp_format_range_to_text_for_editor_service() {
    let txt_file_path = PathBuf::from("/file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&txt_file_path, "")
      .initialize()
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);
          let format_range = |range: std::ops::Range<usize>| {
            let communicator = communicator.clone();
            let txt_file_path = txt_file_path.clone();
            async move {
              let result = communicator
                .format_text(
                  &txt_file_path,
                  "testing".to_string().into_bytes(),
                  Some(range),
                  Default::default(),
                  Default::default(),
                )
                .await;
              String::from_utf8(result.unwrap().unwrap()).unwrap()
            }
          };

          // end past the end of the text
          assert_eq!(format_range(2..100).await, format_range(2..7).await);
          // start and end past the end of the text
          assert_eq!(format_range(50..100).await, format_range(7..7).await);
          // start after the end
          assert_eq!(format_range(5..2).await, format_range(2..2).await);
          // the whole text once clamped
          assert_eq!(format_range(0..100).await, "testing_formatted");

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_respond_to_format_cancelled_by_config_change_for_editor_service() {
    let txt_file_path = PathBuf::from("/file.txt");
    let config_file_path = PathBuf::from("/dprint.json");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&txt_file_path, "")
      .initialize()
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      let environment = environment.clone();
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);
          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);

          // format that doesn't finish until it's cancelled
          let handle = dprint_core::async_runtime::spawn({
            let communicator = communicator.clone();
            let txt_file_path = txt_file_path.clone();
            async move {
              communicator
                .format_text(
                  &txt_file_path,
                  "wait_cancellation".to_string().into_bytes(),
                  None,
                  Default::default(),
                  Default::default(),
                )
                .await
            }
          });

          // give some time for the message to be sent
          tokio::time::sleep(Duration::from_millis(50)).await;

          // the service notices the config change on the next check
          let config_file_text = environment.read_file(&config_file_path).unwrap();
          environment
            .write_file(
              &config_file_path,
              &config_file_text.replacen('{', r#"{ "test-plugin": { "ending": "new_ending" },"#, 1),
            )
            .unwrap();
          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);

          let err = handle.await.unwrap().err().unwrap();
          assert_eq!(err.to_string(), "Formatting was cancelled because the configuration changed.");

          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_new_ending"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_respond_while_resolving_config_for_editor_service() {
    let plugin_url = "https://plugins.dprint.dev/test-plugin.wasm";
    let txt_file_path = PathBuf::from("/file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&txt_file_path, "")
      .build();
    // resolving the config doesn't finish until the plugin is downloaded
    environment.add_unresponsive_remote_file(plugin_url);
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      let environment = environment.clone();
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);
          let check_file_finished = Rc::new(std::cell::Cell::new(false));
          let check_file_handle = dprint_core::async_runtime::spawn({
            let communicator = communicator.clone();
            let txt_file_path = txt_file_path.clone();
            let check_file_finished = check_file_finished.clone();
            async move {
              let result = communicator.check_file(&txt_file_path).await;
              check_file_finished.set(true);
              result
            }
          });

          // give some time for the message to be sent
          tokio::time::sleep(Duration::from_millis(50)).await;

          // responds while the config is being resolved
          communicator.is_alive().await.unwrap();
          assert!(!check_file_finished.get());

          // a format can be cancelled while it's waiting on the config
          let token = CancellationToken::new();
          let format_handle = dprint_core::async_runtime::spawn({
            let communicator = communicator.clone();
            let txt_file_path = txt_file_path.clone();
            let token = token.clone();
            async move {
              communicator
                .format_text(&txt_file_path, "wait_cancellation".to_string().into_bytes(), None, Default::default(), token)
                .await
            }
          });
          tokio::time::sleep(Duration::from_millis(50)).await;
          token.cancel();
          assert_eq!(format_handle.await.unwrap().unwrap(), None);
          communicator.is_alive().await.unwrap();

          environment.remove_unresponsive_remote_file(plugin_url);
          assert_eq!(check_file_handle.await.unwrap().unwrap(), true);
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_formatted"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );

    result.join().unwrap();
  }

  #[test]
  fn should_format_for_editor_service_with_global_config() {
    let txt_file_path = PathBuf::from("/file.txt");
    let ignored_file_path = PathBuf::from("/ignored_file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt").add_excludes("ignored_file.txt");
      })
      .write_file(&txt_file_path, "")
      .write_file(&ignored_file_path, "text")
      .initialize()
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          // Verify that global config is being used
          assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
          assert_eq!(communicator.check_file(&ignored_file_path).await.unwrap(), false);

          // Test formatting with global config
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_formatted"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid, "--config-discovery=global"], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_format_shebang_file_for_editor_service() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_includes("**/*.txt").add_config_section(
          "shebangs",
          r##"{
            "#!/bin/sh": "txt"
          }"##,
        );
      })
      .write_file("/file.txt", "text")
      .write_file("/scripts/build", "#!/bin/sh\ntext")
      .write_file("/scripts/notes", "text")
      .initialize()
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          assert_eq!(communicator.check_file("/file.txt").await.unwrap(), true);
          assert_eq!(communicator.check_file("/file.asdf").await.unwrap(), false);
          assert_eq!(communicator.check_file("/scripts/build").await.unwrap(), true);
          // an extensionless file with no shebang isn't handled by any plugin
          assert_eq!(communicator.check_file("/scripts/notes").await.unwrap(), false);
          // nor is one that doesn't exist on disk
          assert_eq!(communicator.check_file("/scripts/missing").await.unwrap(), false);

          // formatting resolves the plugin from the shebang in the provided bytes
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text("/scripts/build", b"#!/bin/sh\ntext".to_vec(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "#!/bin/sh\ntext_formatted"
          );
          // no shebang -> no plugin -> no change
          assert_eq!(
            communicator
              .format_text("/scripts/notes", b"text".to_vec(), None, Default::default(), Default::default())
              .await
              .unwrap(),
            None
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_format_with_config_associations_for_editor_service() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt_ps";
    let file_path3 = "/file2.other";
    let file_path4 = "/src/some_file_name";
    let file_path5 = "/src/sub-dir/test-process-plugin-exact-file";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
              "associations": [
                "**/*.{txt,txt_ps}",
                "some_file_name",
                "test-process-plugin-exact-file"
              ],
              "ending": "wasm"
            }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
              "associations": [
                "**/*.{txt,txt_ps,other}",
                "test-process-plugin-exact-file"
              ]
              "ending": "ps"
            }"#,
          )
          .add_includes("**/*");
      })
      .write_file(&file_path1, "")
      .write_file(&file_path2, "")
      .write_file(&file_path3, "")
      .write_file(&file_path4, "")
      .write_file(&file_path5, "")
      .build();
    crate::test_helpers::allow_process_plugins_in_editors(&environment);

    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          assert_eq!(communicator.check_file(&file_path1).await.unwrap(), true);
          assert_eq!(communicator.check_file(&file_path2).await.unwrap(), true);
          assert_eq!(communicator.check_file(&file_path3).await.unwrap(), true);
          assert_eq!(communicator.check_file(&file_path4).await.unwrap(), true);
          assert_eq!(communicator.check_file(&file_path5).await.unwrap(), true);

          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&file_path1, "text".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "text_wasm_ps"
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(
                  &file_path1,
                  "plugin: text6".to_string().into_bytes(),
                  None,
                  Default::default(),
                  Default::default()
                )
                .await
                .unwrap()
                .unwrap()
            ),
            "plugin: text6_wasm_ps_wasm_ps_ps"
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&file_path2, "text".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "text_wasm_ps"
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&file_path3, "text".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "text_ps"
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&file_path4, "text".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "text_wasm"
          );
          assert_eq!(
            bytes_to_string(
              communicator
                .format_text(&file_path5, "text".to_string().into_bytes(), None, Default::default(), Default::default())
                .await
                .unwrap()
                .unwrap()
            ),
            "text_wasm_ps"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    run_test_cli(vec!["editor-service", "--parent-pid", &std::process::id().to_string()], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_format_with_config_overrides_for_editor_service() {
    let file_path = "/package.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
            "ending": "base",
            "overrides": {
              "files": "**/package.txt",
              "ending": "package"
            }
          }"#,
        );
      })
      .write_file(file_path, "")
      .build();

    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);

          assert_eq!(communicator.check_file(&file_path).await.unwrap(), true);
          assert_eq!(
            communicator
              .format_text(&file_path, "testing".to_string().into_bytes(), None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            b"testing_package"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }

  #[test]
  fn should_ensure_stable_format_for_editor_service() {
    // formats once by default
    assert_eq!(
      format_unstable_text_with_editor_service(None).unwrap().unwrap(),
      b"unstable_fmt_false_formatted"
    );
    // formats again until the output is stable, like `dprint fmt`, when opted in
    assert_eq!(
      format_unstable_text_with_editor_service(Some("1")).err().unwrap().to_string(),
      "Formatting not stable. Bailed after 5 tries. This indicates a bug in the plugin where it formats the file differently each time."
    );
  }

  fn format_unstable_text_with_editor_service(stable_format_env_var: Option<&str>) -> FormatResult {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(file_path, "")
      .build();
    environment.set_env_var("DPRINT_EDITOR_STABLE_FORMAT", stable_format_env_var);

    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      move || {
        TestEnvironment::new().run_in_runtime(async move {
          let communicator = EditorServiceCommunicator::new(stdin, stdout);
          let result = communicator
            .format_text(&file_path, b"unstable_fmt_true".to_vec(), None, Default::default(), Default::default())
            .await;
          communicator.exit().await.unwrap();
          result
        })
      }
    });

    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap()
  }

  fn bytes_to_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
  }
}
