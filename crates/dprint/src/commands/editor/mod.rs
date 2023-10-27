use anyhow::anyhow;
use anyhow::Result;
use dprint_core::communication::IdGenerator;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use dprint_core::communication::RcIdStore;
use dprint_core::communication::SingleThreadMessageWriter;
use dprint_core::plugins::HostFormatRequest;
use std::io::ErrorKind;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use dprint_core::plugins::process::start_parent_process_checker_task;

mod messages;

use crate::arg_parser::CliArgs;
use crate::arg_parser::EditorServiceSubCommand;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::resolve_config_from_args;
use crate::configuration::ResolvedConfig;
use crate::environment::Environment;
use crate::patterns::FileMatcher;
use crate::plugins::PluginResolver;
use crate::resolution::get_plugins_scope_from_args;
use crate::resolution::resolve_plugins_scope;
use crate::resolution::PluginsScope;
use crate::utils::Semaphore;

use self::messages::EditorMessage;
use self::messages::EditorMessageBody;

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

  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorPluginInfo {
    name: String,
    version: String,
    config_key: String,
    file_extensions: Vec<String>,
    file_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_schema_url: Option<String>,
    help_url: String,
  }

  let mut plugins = Vec::new();
  let scope = get_plugins_scope_from_args(args, environment, plugin_resolver).await?;

  scope.ensure_no_global_config_diagnostics()?;

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
    });
  }

  environment.log_machine_readable(&serde_json::to_string(&EditorInfo {
    schema_version: 5,
    cli_version: environment.cli_version(),
    config_schema_url: "https://dprint.dev/schemas/v0.json".to_string(),
    plugins,
  })?);

  Ok(())
}

pub async fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  editor_service_cmd: &EditorServiceSubCommand,
) -> Result<()> {
  // poll for the existence of the parent process and terminate this process when that process no longer exists
  start_parent_process_checker_task(editor_service_cmd.parent_pid);

  let mut editor_service = EditorService::new(args, environment, plugin_resolver);
  editor_service.run().await
}

struct EditorContext {
  pub id_generator: IdGenerator,
  pub writer: SingleThreadMessageWriter<EditorMessage>,
  pub cancellation_tokens: RcIdStore<Arc<CancellationToken>>,
}

struct EditorService<'a, TEnvironment: Environment> {
  args: &'a CliArgs,
  environment: &'a TEnvironment,
  plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>,
  plugins_scope: Option<Rc<PluginsScope<TEnvironment>>>,
  context: Rc<EditorContext>,
  concurrency_limiter: Rc<Semaphore>,
  config_semaphore: Rc<Semaphore>,
}

impl<'a, TEnvironment: Environment> EditorService<'a, TEnvironment> {
  pub fn new(args: &'a CliArgs, environment: &'a TEnvironment, plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>) -> Self {
    let stdout = environment.stdout();
    let writer = SingleThreadMessageWriter::for_stdout(MessageWriter::new(stdout));
    let max_cores = environment.max_threads();
    let concurrency_limiter = Rc::new(Semaphore::new(std::cmp::max(1, max_cores - 1)));

    Self {
      args,
      environment,
      plugin_resolver,
      plugins_scope: None,
      context: Rc::new(EditorContext {
        id_generator: Default::default(),
        cancellation_tokens: Default::default(),
        writer,
      }),
      concurrency_limiter,
      config_semaphore: Rc::new(Semaphore::new(1)),
    }
  }

  pub async fn run(&mut self) -> Result<()> {
    let environment = self.environment.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<EditorMessage>();
    dprint_core::async_runtime::spawn_blocking(move || {
      let stdin = environment.stdin();
      let mut reader = MessageReader::new(stdin);
      loop {
        let read_message = match EditorMessage::read(&mut reader) {
          Ok(message) => message,
          Err(err) if err.kind() == ErrorKind::BrokenPipe => {
            return;
          }
          Err(err) => {
            log_error!(environment, "Editor service failed reading from stdin: {:#}", err);
            return;
          }
        };
        if tx.send(read_message).is_err() {
          return; // channel disconnected
        }
      }
    });
    loop {
      let Some(message) = rx.recv().await else {
        return Ok(())
      };
      match message.body {
        EditorMessageBody::Success(_message_id) => {}
        EditorMessageBody::Error(_message_id, _data) => {}
        EditorMessageBody::Close => {
          handle_message(&self.context, message.id, || Ok(EditorMessageBody::Success(message.id)));
          return Ok(());
        }
        EditorMessageBody::IsAlive => {
          handle_message(&self.context, message.id, || Ok(EditorMessageBody::Success(message.id)));
        }
        EditorMessageBody::CanFormat(path) => {
          // do this on the same thread
          let result = self.can_format(&path).await;
          handle_message(&self.context, message.id, || {
            result.map(|val| EditorMessageBody::CanFormatResponse(message.id, if val { 1 } else { 0 }))
          });
        }
        EditorMessageBody::CanFormatResponse(_, _) => {
          send_error_response(&self.context, message.id, anyhow!("CLI cannot handle a CanFormatResponse message."));
        }
        EditorMessageBody::Format(body) => {
          if self.plugins_scope.is_none() {
            self.ensure_latest_config().await?;
          }
          let token = Arc::new(CancellationToken::new());
          let request = HostFormatRequest {
            file_path: body.file_path,
            range: body.range,
            override_config: if body.override_config.is_empty() {
              Default::default()
            } else {
              match serde_json::from_slice(&body.override_config) {
                Ok(config) => config,
                Err(err) => {
                  send_error_response(&self.context, message.id, anyhow!("Error deserializing override config. {:#}", err));
                  continue;
                }
              }
            },
            file_text: match String::from_utf8(body.file_text) {
              Ok(text) => text,
              Err(err) => {
                send_error_response(&self.context, message.id, anyhow!("Error decoding text to utf8. {:#}", err));
                continue;
              }
            },
            token: token.clone(),
          };

          let token_storage_guard = self.context.cancellation_tokens.store_with_owned_guard(message.id, token.clone());
          let context = self.context.clone();
          let concurrency_limiter = self.concurrency_limiter.clone();
          let scope = self.plugins_scope.clone().unwrap();
          let _ignore = dprint_core::async_runtime::spawn(async move {
            let _permit = concurrency_limiter.acquire().await;
            if token.is_cancelled() {
              return;
            }

            let result = scope.format(request).await;
            drop(token_storage_guard);
            if token.is_cancelled() {
              return;
            }

            let body = match result {
              Ok(text) => EditorMessageBody::FormatResponse(message.id, text.map(|t| t.into_bytes())),
              Err(err) => EditorMessageBody::Error(message.id, format!("{:#}", err).into_bytes()),
            };
            send_response_body(&context, body);
          });
        }
        EditorMessageBody::FormatResponse(_, _) => {
          send_error_response(&self.context, message.id, anyhow!("CLI cannot handle a FormatResponse message."));
        }
        EditorMessageBody::CancelFormat(message_id) => {
          if let Some(token) = self.context.cancellation_tokens.take(message_id) {
            token.cancel();
          }
        }
        EditorMessageBody::Unknown(message_kind, _) => {
          send_error_response(&self.context, message.id, anyhow!("Unknown message with kind: {}", message_kind));
        }
      }
    }
  }

  async fn can_format(&mut self, file_path: &Path) -> Result<bool> {
    let config = self.ensure_latest_config().await?;

    let file_matcher = FileMatcher::new(&config, &FilePatternArgs::default(), self.environment)?;
    // canonicalize the file path, then check if it's in the list of file paths.
    let resolved_file_path = self.environment.canonicalize(file_path)?;
    log_debug!(self.environment, "Checking can format: {}", resolved_file_path.display());
    Ok(file_matcher.matches_and_dir_not_ignored(&resolved_file_path))
  }

  async fn ensure_latest_config(&mut self) -> Result<Rc<ResolvedConfig>> {
    let _update_permit = self.config_semaphore.acquire().await;
    let config = Rc::new(resolve_config_from_args(self.args, self.environment).await?);

    let last_config = self.plugins_scope.as_ref().and_then(|scope| scope.config.as_ref());
    let has_config_changed = last_config.is_none() || last_config.unwrap() != &config || self.plugins_scope.is_none();
    if has_config_changed {
      self.plugins_scope.take();
      let tokens = self.context.cancellation_tokens.take_all();
      for token in tokens.values() {
        token.cancel();
      }
      self.plugin_resolver.clear_and_shutdown_initialized().await;

      let scope = resolve_plugins_scope(config.clone(), self.environment, self.plugin_resolver).await?;
      scope.ensure_no_global_config_diagnostics()?;
      self.plugins_scope = Some(Rc::new(scope));
    }

    Ok(self.plugins_scope.as_ref().unwrap().config.clone().unwrap())
  }
}

fn handle_message(context: &EditorContext, original_message_id: u32, action: impl FnOnce() -> Result<EditorMessageBody>) {
  match action() {
    Ok(body) => send_response_body(context, body),
    Err(err) => send_error_response(context, original_message_id, err),
  };
}

fn send_error_response(context: &EditorContext, original_message_id: u32, err: anyhow::Error) {
  let body = EditorMessageBody::Error(original_message_id, format!("{:#}", err).into_bytes());
  send_response_body(context, body)
}

fn send_response_body(context: &EditorContext, body: EditorMessageBody) {
  let message = EditorMessage {
    id: context.id_generator.next(),
    body,
  };
  if let Err(err) = context.writer.send(message) {
    panic!("Receiver dropped. {:#}", err);
  }
}

#[cfg(test)]
mod test {
  use anyhow::anyhow;
  use anyhow::Result;
  use dprint_core::async_runtime::future;
  use dprint_core::async_runtime::DropGuardAction;
  use dprint_core::communication::IdGenerator;
  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::RcIdStore;
  use dprint_core::communication::SingleThreadMessageWriter;
  use dprint_core::configuration::ConfigKeyMap;
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
    run_test_cli(vec!["editor-info"], &environment).unwrap();
    let mut final_output = r#"{"schemaVersion":5,"cliVersion":""#.to_string();
    final_output.push_str(&environment.cli_version());
    final_output.push_str(r#"","configSchemaUrl":"https://dprint.dev/schemas/v0.json","plugins":["#);
    final_output
      .push_str(r#"{"name":"test-plugin","version":"0.1.0","configKey":"test-plugin","fileExtensions":["txt"],"fileNames":[],"configSchemaUrl":"https://plugins.dprint.dev/test/schema.json","helpUrl":"https://dprint.dev/plugins/test"},"#);
    final_output.push_str(r#"{"name":"test-process-plugin","version":"0.1.0","configKey":"testProcessPlugin","fileExtensions":["txt_ps"],"fileNames":["test-process-plugin-exact-file"],"helpUrl":"https://dprint.dev/plugins/test-process"}]}"#);
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
      file_text: &str,
      range: FormatRange,
      override_config: ConfigKeyMap,
      token: CancellationToken,
    ) -> FormatResult {
      let (tx, rx) = oneshot::channel::<Result<Option<Vec<u8>>>>();

      let result = self
        .send_message(
          EditorMessageBody::Format(FormatEditorMessageBody {
            file_path: file_path.as_ref().to_path_buf(),
            file_text: file_text.to_string().into_bytes(),
            override_config: serde_json::to_vec(&override_config).unwrap(),
            range,
          }),
          MessageResponseChannel::Format(tx),
          rx,
          Arc::new(token),
        )
        .await;
      result.map(|maybe_text| maybe_text.map(|bytes| String::from_utf8(bytes).unwrap()))
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
              .format_text(&txt_file_path, "testing", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "testing_formatted"
          );
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "testing_formatted", None, Default::default(), Default::default())
              .await
              .unwrap()
              .is_none(),
            true
          ); // it is already formatted
          assert_eq!(
            communicator
              .format_text(&other_ext_path, "testing", None, Default::default(), Default::default())
              .await
              .unwrap()
              .is_none(),
            true
          ); // can't format
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "plugin: format this text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "plugin: format this text_formatted_process_formatted"
          );
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "should_error", None, Default::default(), Default::default())
              .await
              .err()
              .unwrap()
              .to_string(),
            "Did error."
          );
          assert_eq!(
            communicator
              .format_text(&txt_file_path, "plugin: should_error", None, Default::default(), Default::default())
              .await
              .err()
              .unwrap()
              .to_string(),
            "Did error."
          );
          assert_eq!(
            communicator
              .format_text(&PathBuf::from("/file.txt_ps"), "testing", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
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
                  communicator
                    .format_text(&txt_file_path, "testing", None, Default::default(), Default::default())
                    .await
                    .unwrap()
                    .unwrap(),
                  "testing_formatted"
                );
              }
            }));
            handles.push(dprint_core::async_runtime::spawn({
              let communicator = communicator.clone();
              async move {
                assert_eq!(
                  communicator
                    .format_text(&PathBuf::from("/file.txt_ps"), "testing", None, Default::default(), Default::default())
                    .await
                    .unwrap()
                    .unwrap(),
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

          // test range formatting
          assert_eq!(
            communicator
              .format_text(&PathBuf::from("/file.txt_ps"), "testing", Some(1..2), Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "t_formatted_process_sting_formatted_process"
          );

          // test cancellation
          let token = CancellationToken::new();
          let handle = dprint_core::async_runtime::spawn({
            let communicator = communicator.clone();
            let token = token.clone();
            async move {
              assert_eq!(
                communicator
                  .format_text(&PathBuf::from("/file.txt_ps"), "wait_cancellation", None, Default::default(), token)
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

          // test override config
          assert_eq!(
            communicator
              .format_text(
                &PathBuf::from("/file.txt_ps"),
                "testing",
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
              .unwrap(),
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
            communicator
              .format_text(&txt_file_path, "testing", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
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
            communicator
              .format_text(&file_path1, "text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "text_wasm_ps"
          );
          assert_eq!(
            communicator
              .format_text(&file_path1, "plugin: text6", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "plugin: text6_wasm_ps_wasm_ps_ps"
          );
          assert_eq!(
            communicator
              .format_text(&file_path2, "text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "text_wasm_ps"
          );
          assert_eq!(
            communicator
              .format_text(&file_path3, "text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "text_ps"
          );
          assert_eq!(
            communicator
              .format_text(&file_path4, "text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "text_wasm"
          );
          assert_eq!(
            communicator
              .format_text(&file_path5, "text", None, Default::default(), Default::default())
              .await
              .unwrap()
              .unwrap(),
            "text_wasm_ps"
          );

          communicator.exit().await.unwrap();
        });
      }
    });

    run_test_cli(vec!["editor-service", "--parent-pid", &std::process::id().to_string()], &environment).unwrap();

    result.join().unwrap();
  }
}
