//! The editor service, which an editor extension starts to format files.
//!
//! Each schema version is in its own module and only shares what's in this
//! module and `messages`, so that removing an old schema version is mostly
//! removing its module.

use anyhow::Result;
use dprint_core::communication::IdGenerator;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use dprint_core::communication::RcIdStore;
use dprint_core::communication::SingleThreadMessageWriter;
use std::io::ErrorKind;
use std::rc::Rc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use dprint_core::plugins::process::start_parent_process_checker_task;

mod messages;
mod v5;
mod v6;

pub use v5::output_editor_info;

use crate::arg_parser::CliArgs;
use crate::arg_parser::EditorServiceSchemaVersion;
use crate::arg_parser::EditorServiceSubCommand;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::resolution::PluginsScope;
use crate::utils::Semaphore;

use self::messages::EditorMessage;
use self::messages::EditorPluginInfo;
use self::messages::MessageBody;

const CONFIG_SCHEMA_URL: &str = "https://dprint.dev/schemas/v0.json";

/// Outputs what the editor service supports (`dprint editor-service --info`),
/// which an editor uses to choose the schema version to start it with. This
/// doesn't need a config file.
///
/// An older version of dprint doesn't have the `--info` flag, so it fails. An
/// editor that also supports schema version 5 can then use `dprint editor-info`.
pub fn output_editor_service_info(environment: &impl Environment) -> Result<()> {
  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorServiceInfo {
    cli_version: String,
    editor_service_schema_versions: Vec<u32>,
    config_schema_url: &'static str,
    /// What the editor service supports in addition to its schema version.
    capabilities: EditorServiceCapabilities,
  }

  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorServiceCapabilities {}

  environment.log_machine_readable(&serde_json::to_vec(&EditorServiceInfo {
    cli_version: environment.cli_version(),
    editor_service_schema_versions: EditorServiceSchemaVersion::ALL.iter().map(|version| version.as_u32()).collect(),
    config_schema_url: CONFIG_SCHEMA_URL,
    capabilities: EditorServiceCapabilities {},
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

  match editor_service_cmd.schema_version {
    EditorServiceSchemaVersion::V5 => v5::run_editor_service(args, environment, plugin_resolver).await,
    EditorServiceSchemaVersion::V6 => v6::run_editor_service(args, environment, plugin_resolver).await,
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

/// What's used to send messages of a schema version to the editor.
struct EditorContext<TBody: MessageBody> {
  pub id_generator: IdGenerator,
  pub writer: SingleThreadMessageWriter<EditorMessage<TBody>>,
  pub cancellation_tokens: RcIdStore<Arc<CancellationToken>>,
  /// Cancelled when a response can't be sent because the editor closed
  /// the service's output, which is when the service should exit.
  pub output_closed: CancellationToken,
}

impl<TBody: MessageBody> EditorContext<TBody> {
  pub fn new(environment: &impl Environment) -> Self {
    let stdout = environment.stdout();
    Self {
      id_generator: Default::default(),
      // this is the writer that doesn't panic when writing fails, which
      // happens when the editor closes the service's output
      writer: SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdout)),
      cancellation_tokens: Default::default(),
      output_closed: Default::default(),
    }
  }
}

/// Reads the messages of a schema version from the editor on another thread.
fn start_reading_messages<TBody: MessageBody>(environment: &impl Environment) -> tokio::sync::mpsc::UnboundedReceiver<EditorMessage<TBody>> {
  let environment = environment.clone();
  let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<EditorMessage<TBody>>();
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
  rx
}

/// Limits how many files are formatted at a time so the editor service
/// doesn't use every core.
fn new_format_concurrency_limiter(environment: &impl Environment) -> Semaphore {
  Semaphore::new(std::cmp::max(1, environment.max_threads() - 1))
}

fn send_error_response<TBody: MessageBody>(context: &EditorContext<TBody>, original_message_id: u32, err: anyhow::Error) {
  let body = TBody::error(original_message_id, format!("{:#}", err).into_bytes());
  send_response_body(context, body)
}

fn send_response_body<TBody: MessageBody>(context: &EditorContext<TBody>, body: TBody) {
  let message = EditorMessage {
    id: context.id_generator.next(),
    body,
  };
  if context.writer.send(message).is_err() {
    context.output_closed.cancel();
  }
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn should_output_editor_service_info_without_config_file() {
    let environment = TestEnvironmentBuilder::new().build();
    run_test_cli(vec!["editor-service", "--info"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![format!(
        r#"{{"cliVersion":"{}","editorServiceSchemaVersions":[5,6],"configSchemaUrl":"https://dprint.dev/schemas/v0.json","capabilities":{{}}}}"#,
        environment.cli_version()
      )]
    );
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn should_not_start_schema_version_that_does_not_exist() {
    let environment = TestEnvironment::new();
    let err = run_test_cli(vec!["editor-service", "--parent-pid", "1", "--schema-version", "7"], &environment).unwrap_err();
    err.assert_exit_code(10);
    assert!(err.to_string().contains("[possible values: 5, 6]"), "{err:#}");
  }
}
