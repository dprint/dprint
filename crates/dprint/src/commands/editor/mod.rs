use anyhow::anyhow;
use anyhow::Result;
use dprint_core::communication::ArcIdStore;
use dprint_core::communication::IdGenerator;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use dprint_core::communication::SingleThreadMessageWriter;
use dprint_core::plugins::Host;
use dprint_core::plugins::HostFormatRequest;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use dprint_core::plugins::process::start_parent_process_checker_task;

mod messages;

use crate::arg_parser::CliArgs;
use crate::arg_parser::EditorServiceSubCommand;
use crate::arg_parser::FilePatternArgs;
use crate::cache::Cache;
use crate::configuration::resolve_config_from_args;
use crate::configuration::ResolvedConfig;
use crate::environment::Environment;
use crate::patterns::FileMatcher;
use crate::plugins::get_plugins_from_args;
use crate::plugins::resolve_plugins;
use crate::plugins::PluginResolver;
use crate::plugins::PluginsCollection;

use self::messages::EditorMessage;
use self::messages::EditorMessageBody;

pub async fn output_editor_info<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
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

  for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver).await? {
    plugins.push(EditorPluginInfo {
      name: plugin.name().to_string(),
      version: plugin.version().to_string(),
      config_key: plugin.config_key().to_string(),
      file_extensions: plugin.file_extensions().iter().map(|ext| ext.to_string()).collect(),
      file_names: plugin.file_names().iter().map(|ext| ext.to_string()).collect(),
      config_schema_url: if plugin.config_schema_url().trim().is_empty() {
        None
      } else {
        Some(plugin.config_schema_url().trim().to_string())
      },
      help_url: plugin.help_url().to_string(),
    });
  }

  environment.log_machine_readable(&serde_json::to_string(&EditorInfo {
    schema_version: 5,
    cli_version: env!("CARGO_PKG_VERSION").to_string(),
    config_schema_url: "https://dprint.dev/schemas/v0.json".to_string(),
    plugins,
  })?);

  Ok(())
}

pub async fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginsCollection<TEnvironment>>,
  editor_service_cmd: &EditorServiceSubCommand,
) -> Result<()> {
  // poll for the existence of the parent process and terminate this process when that process no longer exists
  let _handle = start_parent_process_checker_task(editor_service_cmd.parent_pid);

  let mut editor_service = EditorService::new(args, cache, environment, plugin_resolver, plugin_pools);
  editor_service.run().await
}

struct EditorContext {
  pub id_generator: IdGenerator,
  pub writer: SingleThreadMessageWriter<EditorMessage>,
  pub cancellation_tokens: ArcIdStore<Arc<CancellationToken>>,
}

struct EditorService<'a, TEnvironment: Environment> {
  reader: MessageReader<Box<dyn Read + Send>>,
  config: Option<ResolvedConfig>,
  args: &'a CliArgs,
  cache: &'a Cache<TEnvironment>,
  environment: &'a TEnvironment,
  plugin_resolver: &'a PluginResolver<TEnvironment>,
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  context: Arc<EditorContext>,
  concurrency_limiter: Arc<Semaphore>,
}

impl<'a, TEnvironment: Environment> EditorService<'a, TEnvironment> {
  pub fn new(
    args: &'a CliArgs,
    cache: &'a Cache<TEnvironment>,
    environment: &'a TEnvironment,
    plugin_resolver: &'a PluginResolver<TEnvironment>,
    plugin_pools: Arc<PluginsCollection<TEnvironment>>,
  ) -> Self {
    let stdin = environment.stdin();
    let stdout = environment.stdout();
    let reader = MessageReader::new(stdin);
    let writer = SingleThreadMessageWriter::for_stdout(MessageWriter::new(stdout));
    let number_cores = environment.available_parallelism();
    let concurrency_limiter = Arc::new(Semaphore::new(std::cmp::max(1, number_cores - 1)));

    Self {
      reader,
      config: None,
      args,
      cache,
      environment,
      plugin_resolver,
      plugins_collection: plugin_pools,
      context: Arc::new(EditorContext {
        id_generator: Default::default(),
        cancellation_tokens: Default::default(),
        writer,
      }),
      concurrency_limiter,
    }
  }

  pub async fn run(&mut self) -> Result<()> {
    loop {
      let message = EditorMessage::read(&mut self.reader)?;
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
          if self.config.is_none() {
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

          self.context.cancellation_tokens.store(message.id, token.clone());
          let plugins_collection = self.plugins_collection.clone();
          let context = self.context.clone();
          let concurrency_limiter = self.concurrency_limiter.clone();
          let _ignore = tokio::task::spawn(async move {
            let _permit = concurrency_limiter.acquire().await;
            if token.is_cancelled() {
              return;
            }

            let result = plugins_collection.format(request).await;
            context.cancellation_tokens.take(message.id);
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
    self.ensure_latest_config().await?;

    let file_matcher = FileMatcher::new(self.config.as_ref().unwrap(), &FilePatternArgs::default(), self.environment)?;
    // canonicalize the file path, then check if it's in the list of file paths.
    let resolved_file_path = self.environment.canonicalize(&file_path)?;
    log_verbose!(self.environment, "Checking can format: {}", resolved_file_path.display());
    Ok(file_matcher.matches(&resolved_file_path))
  }

  async fn ensure_latest_config(&mut self) -> Result<()> {
    let last_config = self.config.take();
    let config = resolve_config_from_args(self.args, self.cache, self.environment)?;

    let has_config_changed = last_config.is_none() || last_config.unwrap() != config;
    if has_config_changed {
      self.plugins_collection.drop_and_shutdown_initialized().await; // clear the existing plugins
      let plugins = resolve_plugins(self.args, &config, self.environment, self.plugin_resolver).await?;
      self.plugins_collection.set_plugins(plugins, &config.base_path)?;
    }

    self.config = Some(config);

    Ok(())
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
  use dprint_core::communication::ArcIdStore;
  use dprint_core::communication::IdGenerator;
  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::Poisoner;
  use dprint_core::communication::SingleThreadMessageWriter;
  use dprint_core::configuration::ConfigKeyMap;
  use dprint_core::plugins::FormatRange;
  use dprint_core::plugins::FormatResult;
  use pretty_assertions::assert_eq;
  use std::io::Read;
  use std::io::Write;
  use std::path::Path;
  use std::path::PathBuf;
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
    final_output.push_str(&env!("CARGO_PKG_VERSION").to_string());
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
    writer: SingleThreadMessageWriter<EditorMessage>,
    id_generator: IdGenerator,
    messages: ArcIdStore<MessageResponseChannel>,
  }

  impl EditorServiceCommunicator {
    pub fn new(stdin: Box<dyn Write + Send>, stdout: Box<dyn Read + Send>) -> Self {
      let mut reader = MessageReader::new(stdout);
      let writer = SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdin), Poisoner::default());

      let communicator = EditorServiceCommunicator {
        writer,
        id_generator: Default::default(),
        messages: Default::default(),
      };

      tokio::task::spawn_blocking({
        let messages = communicator.messages.clone();
        move || loop {
          if let Err(_) = read_stdout_message(&mut reader, &messages) {
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
      self.messages.store(message_id, response_channel);
      self.writer.send(EditorMessage { id: message_id, body })?;
      tokio::select! {
        _ = token.cancelled() => {
          self.writer.send(EditorMessage { id: self.id_generator.next(), body: EditorMessageBody::CancelFormat(message_id) })?;
          self.messages.take(message_id); // clear memory
          Ok(Default::default())
        }
        response = receiver => {
          match response {
            Ok(data) => data,
            Err(err) => panic!("{:#}", err)
          }
        }
      }
    }
  }

  fn read_stdout_message(reader: &mut MessageReader<Box<dyn Read + Send>>, messages: &ArcIdStore<MessageResponseChannel>) -> Result<()> {
    let message = EditorMessage::read(reader)?;

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
          .add_excludes("ignored_file.txt");
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
            handles.push(tokio::task::spawn({
              let communicator = communicator.clone();
              let txt_file_path = txt_file_path.clone();
              async move {
                assert_eq!(communicator.check_file(&txt_file_path).await.unwrap(), true);
              }
            }));
            handles.push(tokio::task::spawn({
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
            handles.push(tokio::task::spawn({
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
          let results = futures::future::join_all(handles).await;
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
          let handle = tokio::task::spawn({
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
