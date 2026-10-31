//! The editor service, which an editor extension starts to format files with
//! the config file the editor found. Formatting the files of a whole
//! workspace is done by the language server instead (see `commands::lsp`).

use anyhow::Result;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use dprint_core::communication::RcIdStore;
use dprint_core::communication::SingleThreadMessageWriter;
use std::io::ErrorKind;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

use dprint_core::plugins::process::start_parent_process_checker_task;

mod messages;
mod v5;

pub use v5::output_editor_info;

use crate::arg_parser::CliArgs;
use crate::arg_parser::EditorServiceSubCommand;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::utils::Semaphore;

use self::messages::EditorMessage;
use self::messages::MessageBody;

const CONFIG_SCHEMA_URL: &str = "https://dprint.dev/schemas/v0.json";

pub async fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  editor_service_cmd: &EditorServiceSubCommand,
) -> Result<()> {
  // poll for the existence of the parent process and terminate this process when that process no longer exists
  start_parent_process_checker_task(editor_service_cmd.parent_pid);

  v5::run_editor_service(args, environment, plugin_resolver).await
}

/// What's used to send messages of a schema version to the editor.
struct EditorContext<TBody: MessageBody> {
  pub id_generator: Arc<AtomicIdGenerator>,
  pub writer: Arc<SingleThreadMessageWriter<EditorMessage<TBody>>>,
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
      writer: Arc::new(SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdout))),
      cancellation_tokens: Default::default(),
      output_closed: Default::default(),
    }
  }
}

/// Generates the ids of the messages sent to the editor, which can be done
/// from any thread.
#[derive(Default)]
struct AtomicIdGenerator(AtomicU32);

impl AtomicIdGenerator {
  pub fn next(&self) -> u32 {
    self.0.fetch_add(1, Ordering::Relaxed)
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
  use std::io::Read;
  use std::io::Write;
  use std::sync::Arc;
  use std::time::Duration;

  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::SingleThreadMessageWriter;

  use crate::configuration::get_settings_file_path;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;

  use super::EditorContext;
  use super::messages::MessageBody;
  use super::messages::MessageBuilder;
  use super::send_response_body;

  /// A message body for testing what the schema versions share.
  #[derive(Debug)]
  struct TestMessageBody;

  impl MessageBody for TestMessageBody {
    fn read<TRead: Read + Unpin>(_reader: &mut MessageReader<TRead>, _kind: u32, _body_length: u32) -> std::io::Result<Self> {
      Ok(TestMessageBody)
    }

    fn write<'a>(&'a self, _builder: &mut MessageBuilder<'a>) {}

    fn kind(&self) -> u32 {
      0
    }

    fn error(_message_id: u32, _data: Vec<u8>) -> Self {
      TestMessageBody
    }
  }

  #[test]
  fn should_not_panic_sending_response_when_output_closed() {
    struct ClosedWriter;

    impl Write for ClosedWriter {
      fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Broken pipe."))
      }

      fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
      }
    }

    TestEnvironment::new().run_in_runtime(async move {
      let context = EditorContext::<TestMessageBody> {
        id_generator: Default::default(),
        cancellation_tokens: Default::default(),
        writer: Arc::new(SingleThreadMessageWriter::for_stdin(MessageWriter::new(ClosedWriter))),
        output_closed: Default::default(),
      };
      // the first message fails to be written on the writer thread
      // and then the messages after that can't be sent
      for _ in 0..500 {
        send_response_body(&context, TestMessageBody);
        if context.output_closed.is_cancelled() {
          break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
      assert!(context.output_closed.is_cancelled());
    });
  }

  /// Requires the user to approve running process plugins.
  pub(super) fn disallow_process_plugins(environment: &TestEnvironment) {
    let file_path = get_settings_file_path(environment).unwrap();
    // macOS only uses this directory when it's already there
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment.write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": false } }"#).unwrap();
  }
}
