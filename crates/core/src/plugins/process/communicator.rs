use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context as AnyhowContext;
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::cell::RefCell;
use std::io::BufRead;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStderr;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::messages::CheckConfigUpdatesMessageBody;
use super::messages::CheckConfigUpdatesResponseBody;
use super::messages::FormatMessageBody;
use super::messages::HostFormatMessageBody;
use super::messages::MessageBody;
use super::messages::ProcessPluginMessage;
use super::messages::RegisterConfigMessageBody;
use super::messages::ResponseBody;
use super::PLUGIN_SCHEMA_VERSION;
use crate::async_runtime::DropGuardAction;
use crate::async_runtime::LocalBoxFuture;
use crate::communication::AtomicFlag;
use crate::communication::IdGenerator;
use crate::communication::MessageReader;
use crate::communication::MessageWriter;
use crate::communication::RcIdStore;
use crate::communication::SingleThreadMessageWriter;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::ConfigChange;
use crate::plugins::CriticalFormatError;
use crate::plugins::FileMatchingInfo;
use crate::plugins::FormatConfigId;
use crate::plugins::FormatRange;
use crate::plugins::FormatResult;
use crate::plugins::HostFormatRequest;
use crate::plugins::NullCancellationToken;
use crate::plugins::PluginInfo;

type DprintCancellationToken = Arc<dyn super::super::CancellationToken>;

pub type HostFormatCallback = Rc<dyn Fn(HostFormatRequest) -> LocalBoxFuture<'static, FormatResult>>;

pub struct ProcessPluginCommunicatorFormatRequest {
  pub file_path: PathBuf,
  pub file_bytes: Vec<u8>,
  pub range: FormatRange,
  pub config_id: FormatConfigId,
  pub override_config: ConfigKeyMap,
  pub on_host_format: HostFormatCallback,
  pub token: DprintCancellationToken,
}

enum MessageResponseChannel {
  Acknowledgement(oneshot::Sender<Result<()>>),
  Data(oneshot::Sender<Result<Vec<u8>>>),
  Format(oneshot::Sender<Result<Option<Vec<u8>>>>),
}

struct Context {
  stdin_writer: SingleThreadMessageWriter<ProcessPluginMessage>,
  shutdown_flag: Arc<AtomicFlag>,
  id_generator: IdGenerator,
  messages: RcIdStore<MessageResponseChannel>,
  format_request_tokens: RcIdStore<Arc<CancellationToken>>,
  host_format_callbacks: RcIdStore<HostFormatCallback>,
}

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
  child: RefCell<Option<Child>>,
  context: Rc<Context>,
}

impl Drop for ProcessPluginCommunicator {
  fn drop(&mut self) {
    self.kill();
  }
}

impl ProcessPluginCommunicator {
  pub async fn new(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, false, on_std_err).await
  }

  /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
  pub async fn new_with_init(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, true, on_std_err).await
  }

  async fn new_internal(executable_file_path: &Path, is_init: bool, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
    let mut args = vec!["--parent-pid".to_string(), std::process::id().to_string()];
    if is_init {
      args.push("--init".to_string());
    }

    let shutdown_flag = Arc::new(AtomicFlag::default());
    let mut child = Command::new(executable_file_path)
      .args(&args)
      .stdin(Stdio::piped())
      .stderr(Stdio::piped())
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|err| anyhow!("Error starting {} with args [{}]. {:#}", executable_file_path.display(), args.join(" "), err))?;

    // read and output stderr prefixed
    let stderr = child.stderr.take().unwrap();
    crate::async_runtime::spawn_blocking({
      let shutdown_flag = shutdown_flag.clone();
      let on_std_err = on_std_err.clone();
      move || {
        std_err_redirect(shutdown_flag, stderr, on_std_err);
      }
    });

    // verify the schema version
    let mut stdout_reader = MessageReader::new(child.stdout.take().unwrap());
    let mut stdin_writer = MessageWriter::new(child.stdin.take().unwrap());

    let (mut stdout_reader, stdin_writer, schema_version) = crate::async_runtime::spawn_blocking(move || {
      let schema_version = get_plugin_schema_version(&mut stdout_reader, &mut stdin_writer)
        .context("Failed plugin schema verification. This may indicate you are using an old version of the dprint CLI or plugin and should upgrade")?;
      Ok::<_, anyhow::Error>((stdout_reader, stdin_writer, schema_version))
    })
    .await??;

    if schema_version != PLUGIN_SCHEMA_VERSION {
      // kill the child to prevent it from ouputting to stderr
      let _ = child.kill();
      if schema_version < PLUGIN_SCHEMA_VERSION {
        bail!(
          "This plugin is too old to run in the dprint CLI and you will need to manually upgrade it (version was {}, but expected {}).\n\nUpgrade instructions: https://github.com/dprint/dprint/issues/731",
          schema_version,
          PLUGIN_SCHEMA_VERSION
        );
      } else {
        bail!(
          "Your dprint CLI is too old to run this plugin (version was {}, but expected {}). Try running: dprint upgrade",
          schema_version,
          PLUGIN_SCHEMA_VERSION
        );
      }
    }

    let stdin_writer = SingleThreadMessageWriter::for_stdin(stdin_writer);
    let context = Rc::new(Context {
      id_generator: Default::default(),
      shutdown_flag,
      stdin_writer,
      messages: Default::default(),
      format_request_tokens: Default::default(),
      host_format_callbacks: Default::default(),
    });

    // read from stdout
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    crate::async_runtime::spawn_blocking({
      let shutdown_flag = context.shutdown_flag.clone();
      let on_std_err = on_std_err.clone();
      move || {
        loop {
          match ProcessPluginMessage::read(&mut stdout_reader) {
            Ok(message) => {
              if tx.send(message).is_err() {
                break; // closed
              }
            }
            Err(err) if err.kind() == ErrorKind::BrokenPipe => {
              break;
            }
            Err(err) => {
              if !shutdown_flag.is_raised() {
                on_std_err(format!("Error reading stdout message: {:#}", err));
              }
              break;
            }
          }
        }
      }
    });
    crate::async_runtime::spawn({
      let context = context.clone();
      async move {
        while let Some(message) = rx.recv().await {
          if let Err(err) = handle_stdout_message(message, &context) {
            if !context.shutdown_flag.is_raised() {
              on_std_err(format!("Error reading stdout message: {:#}", err));
            }
            break;
          }
        }
        // clear out all the messages
        context.messages.take_all();
      }
    });

    Ok(Self {
      child: RefCell::new(Some(child)),
      context,
    })
  }

  /// Perform a graceful shutdown.
  pub async fn shutdown(&self) {
    if self.context.shutdown_flag.raise() {
      // attempt to exit nicely
      tokio::select! {
        // we wait for acknowledgement in order to give the process
        // plugin a chance to clean up (ex. in case it has spawned
        // any processes it needs to kill or something like that)
        _ = self.send_with_acknowledgement(MessageBody::Close) => {}
        _ = tokio::time::sleep(Duration::from_millis(250)) => {
          self.kill();
        }
      }
    } else {
      self.kill();
    }
  }

  pub fn kill(&self) {
    self.context.shutdown_flag.raise();
    if let Some(mut child) = self.child.borrow_mut().take() {
      let _ignore = child.kill();
    }
  }

  pub async fn register_config(&self, config_id: FormatConfigId, global_config: &GlobalConfiguration, plugin_config: &ConfigKeyMap) -> Result<()> {
    let global_config = serde_json::to_vec(global_config)?;
    let plugin_config = serde_json::to_vec(plugin_config)?;
    self
      .send_with_acknowledgement(MessageBody::RegisterConfig(RegisterConfigMessageBody {
        config_id,
        global_config,
        plugin_config,
      }))
      .await?;
    Ok(())
  }

  pub async fn release_config(&self, config_id: FormatConfigId) -> Result<()> {
    self.send_with_acknowledgement(MessageBody::ReleaseConfig(config_id)).await?;
    Ok(())
  }

  pub async fn ask_is_alive(&self) -> bool {
    self.send_with_acknowledgement(MessageBody::IsAlive).await.is_ok()
  }

  pub async fn plugin_info(&self) -> Result<PluginInfo> {
    self.send_receiving_data(MessageBody::GetPluginInfo).await
  }

  pub async fn license_text(&self) -> Result<String> {
    self.send_receiving_string(MessageBody::GetLicenseText).await
  }

  pub async fn resolved_config(&self, config_id: FormatConfigId) -> Result<String> {
    self.send_receiving_string(MessageBody::GetResolvedConfig(config_id)).await
  }

  pub async fn file_matching_info(&self, config_id: FormatConfigId) -> Result<FileMatchingInfo> {
    self.send_receiving_data(MessageBody::GetFileMatchingInfo(config_id)).await
  }

  pub async fn config_diagnostics(&self, config_id: FormatConfigId) -> Result<Vec<ConfigurationDiagnostic>> {
    self.send_receiving_data(MessageBody::GetConfigDiagnostics(config_id)).await
  }

  pub async fn check_config_updates(&self, plugin_config: ConfigKeyMap) -> Result<Vec<ConfigChange>> {
    let message = CheckConfigUpdatesMessageBody { config: plugin_config };
    let bytes = serde_json::to_vec(&message)?;
    let response: CheckConfigUpdatesResponseBody = self.send_receiving_data(MessageBody::CheckConfigUpdates(bytes)).await?;
    Ok(response.changes)
  }

  pub async fn format_text(&self, request: ProcessPluginCommunicatorFormatRequest) -> FormatResult {
    let (tx, rx) = oneshot::channel::<Result<Option<Vec<u8>>>>();

    let message_id = self.context.id_generator.next();
    let store_guard = self.context.host_format_callbacks.store_with_guard(message_id, request.on_host_format);
    let maybe_result = self
      .send_message_with_id(
        message_id,
        MessageBody::Format(FormatMessageBody {
          file_path: request.file_path,
          file_bytes: request.file_bytes,
          range: request.range,
          config_id: request.config_id,
          override_config: serde_json::to_vec(&request.override_config).unwrap(),
        }),
        MessageResponseChannel::Format(tx),
        rx,
        request.token.clone(),
      )
      .await;

    drop(store_guard); // explicit for clarity

    if request.token.is_cancelled() {
      Ok(None)
    } else {
      match maybe_result {
        Ok(result) => result,
        Err(err) => Err(CriticalFormatError(err).into()),
      }
    }
  }

  /// Checks if the process is functioning.
  pub async fn is_process_alive(&self) -> bool {
    if self.context.shutdown_flag.is_raised() {
      false
    } else {
      self.ask_is_alive().await
    }
  }

  async fn send_with_acknowledgement(&self, body: MessageBody) -> Result<()> {
    let (tx, rx) = oneshot::channel::<Result<()>>();
    self
      .send_message(body, MessageResponseChannel::Acknowledgement(tx), rx, Arc::new(NullCancellationToken))
      .await?
  }

  async fn send_receiving_string(&self, body: MessageBody) -> Result<String> {
    let data = self.send_receiving_bytes(body).await??;
    Ok(String::from_utf8(data)?)
  }

  async fn send_receiving_data<T: DeserializeOwned>(&self, body: MessageBody) -> Result<T> {
    let data = self.send_receiving_bytes(body).await??;
    Ok(serde_json::from_slice(&data)?)
  }

  async fn send_receiving_bytes(&self, body: MessageBody) -> Result<Result<Vec<u8>>> {
    let (tx, rx) = oneshot::channel::<Result<Vec<u8>>>();
    self
      .send_message(body, MessageResponseChannel::Data(tx), rx, Arc::new(NullCancellationToken))
      .await
  }

  async fn send_message<T: Default>(
    &self,
    body: MessageBody,
    response_channel: MessageResponseChannel,
    receiver: oneshot::Receiver<Result<T>>,
    token: Arc<dyn super::super::CancellationToken>,
  ) -> Result<Result<T>> {
    let message_id = self.context.id_generator.next();
    self.send_message_with_id(message_id, body, response_channel, receiver, token).await
  }

  async fn send_message_with_id<T: Default>(
    &self,
    message_id: u32,
    body: MessageBody,
    response_channel: MessageResponseChannel,
    receiver: oneshot::Receiver<Result<T>>,
    token: Arc<dyn super::super::CancellationToken>,
  ) -> Result<Result<T>> {
    let mut drop_guard = DropGuardAction::new(|| {
      // clear up memory
      self.context.messages.take(message_id);
      // send cancellation to the client
      let _ = self.context.stdin_writer.send(ProcessPluginMessage {
        id: self.context.id_generator.next(),
        body: MessageBody::CancelFormat(message_id),
      });
    });

    self.context.messages.store(message_id, response_channel);
    self.context.stdin_writer.send(ProcessPluginMessage { id: message_id, body })?;
    tokio::select! {
      _ = token.wait_cancellation() => {
        drop(drop_guard); // explicit
        Ok(Ok(Default::default()))
      }
      response = receiver => {
        drop_guard.forget(); // we completed, so don't run the drop guard
        match response {
          Ok(data) => Ok(data),
          Err(err) => {
            bail!("Error waiting on message ({}). {:#}", message_id, err)
          }
        }
      }
    }
  }
}

fn get_plugin_schema_version<TRead: Read + Unpin, TWrite: Write + Unpin>(reader: &mut MessageReader<TRead>, writer: &mut MessageWriter<TWrite>) -> Result<u32> {
  // since this is the setup, use a lot of contexts to find exactly where it failed
  writer.send_u32(0).context("Failed asking for schema version.")?; // ask for schema version
  writer.flush().context("Failed flushing schema version request.")?;
  let acknowledgement_response = reader.read_u32().context("Could not read success response.")?;
  if acknowledgement_response != 0 {
    bail!("Plugin response was unexpected ({acknowledgement_response}).");
  }
  reader.read_u32().context("Could not read schema version.")
}

fn std_err_redirect(shutdown_flag: Arc<AtomicFlag>, stderr: ChildStderr, on_std_err: impl Fn(String) + Send + Sync + 'static) {
  let reader = std::io::BufReader::new(stderr);
  for line in reader.lines() {
    match line {
      Ok(line) => on_std_err(line),
      Err(err) => {
        if shutdown_flag.is_raised() || err.kind() == ErrorKind::BrokenPipe {
          return;
        } else {
          on_std_err(format!("Error reading line from process plugin stderr. {:#}", err));
        }
      }
    }
  }
}

fn handle_stdout_message(message: ProcessPluginMessage, context: &Rc<Context>) -> Result<()> {
  match message.body {
    MessageBody::Success(message_id) => match context.messages.take(message_id) {
      Some(MessageResponseChannel::Acknowledgement(channel)) => {
        let _ignore = channel.send(Ok(()));
      }
      Some(MessageResponseChannel::Data(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected data channel for success response: {}", message_id)));
      }
      Some(MessageResponseChannel::Format(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected format channel for success response: {}", message_id)));
      }
      None => {}
    },
    MessageBody::DataResponse(response) => match context.messages.take(response.message_id) {
      Some(MessageResponseChannel::Acknowledgement(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected success channel for data response: {}", response.message_id)));
      }
      Some(MessageResponseChannel::Data(channel)) => {
        let _ignore = channel.send(Ok(response.data));
      }
      Some(MessageResponseChannel::Format(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected format channel for data response: {}", response.message_id)));
      }
      None => {}
    },
    MessageBody::Error(response) => {
      let err = anyhow!("{}", String::from_utf8_lossy(&response.data));
      match context.messages.take(response.message_id) {
        Some(MessageResponseChannel::Acknowledgement(channel)) => {
          let _ignore = channel.send(Err(err));
        }
        Some(MessageResponseChannel::Data(channel)) => {
          let _ignore = channel.send(Err(err));
        }
        Some(MessageResponseChannel::Format(channel)) => {
          let _ignore = channel.send(Err(err));
        }
        None => {}
      }
    }
    MessageBody::FormatResponse(response) => match context.messages.take(response.message_id) {
      Some(MessageResponseChannel::Acknowledgement(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected success channel for format response: {}", response.message_id)));
      }
      Some(MessageResponseChannel::Data(channel)) => {
        let _ignore = channel.send(Err(anyhow!("Unexpected data channel for format response: {}", response.message_id)));
      }
      Some(MessageResponseChannel::Format(channel)) => {
        let _ignore = channel.send(Ok(response.data));
      }
      None => {}
    },
    MessageBody::CancelFormat(message_id) => {
      if let Some(token) = context.format_request_tokens.take(message_id) {
        token.cancel();
      }
      context.host_format_callbacks.take(message_id);
      // do not clear from context.messages here because the cancellation will do that
    }
    MessageBody::HostFormat(body) => {
      // spawn a task to do the host formatting, then send a message back to the
      // plugin with the result
      let context = context.clone();
      crate::async_runtime::spawn(async move {
        let result = host_format(context.clone(), message.id, body).await;

        // ignore failure, as this means that the process shut down
        // at which point handling would have occurred elsewhere
        let _ignore = context.stdin_writer.send(ProcessPluginMessage {
          id: context.id_generator.next(),
          body: match result {
            Ok(result) => MessageBody::FormatResponse(ResponseBody {
              message_id: message.id,
              data: result,
            }),
            Err(err) => MessageBody::Error(ResponseBody {
              message_id: message.id,
              data: format!("{:#}", err).into_bytes(),
            }),
          },
        });
      });
    }
    MessageBody::IsAlive => {
      // the CLI is not documented as supporting this, but we might as well respond
      let _ = context.stdin_writer.send(ProcessPluginMessage {
        id: context.id_generator.next(),
        body: MessageBody::Success(message.id),
      });
    }
    MessageBody::Format(_)
    | MessageBody::Close
    | MessageBody::GetPluginInfo
    | MessageBody::GetLicenseText
    | MessageBody::RegisterConfig(_)
    | MessageBody::ReleaseConfig(_)
    | MessageBody::GetConfigDiagnostics(_)
    | MessageBody::GetFileMatchingInfo(_)
    | MessageBody::GetResolvedConfig(_)
    | MessageBody::CheckConfigUpdates(_) => {
      let _ = context.stdin_writer.send(ProcessPluginMessage {
        id: context.id_generator.next(),
        body: MessageBody::Error(ResponseBody {
          message_id: message.id,
          data: "Unsupported plugin to CLI message.".as_bytes().to_vec(),
        }),
      });
    }
    // If encountered, process plugin should exit and
    // the CLI should kill the process plugin.
    MessageBody::Unknown(message_kind) => {
      bail!("Unknown message kind: {}", message_kind);
    }
  }

  Ok(())
}

async fn host_format(context: Rc<Context>, message_id: u32, body: HostFormatMessageBody) -> FormatResult {
  let Some(callback) = context.host_format_callbacks.get_cloned(body.original_message_id) else {
    return FormatResult::Err(anyhow!("Could not find host format callback for message id: {}", body.original_message_id));
  };

  let token = Arc::new(CancellationToken::new());
  let store_guard = context.format_request_tokens.store_with_guard(message_id, token.clone());
  let result = callback(HostFormatRequest {
    file_path: body.file_path,
    file_bytes: body.file_text,
    range: body.range,
    override_config: serde_json::from_slice(&body.override_config).unwrap(),
    token,
  })
  .await;
  drop(store_guard); // explicit for clarity
  result
}
