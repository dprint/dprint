use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context as AnyhowContext;
use anyhow::Result;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStderr;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::messages::FormatMessageBody;
use super::messages::HostFormatMessageBody;
use super::messages::MessageBody;
use super::messages::ProcessPluginMessage;
use super::messages::RegisterConfigMessageBody;
use super::messages::ResponseBody;
use super::PLUGIN_SCHEMA_VERSION;
use crate::communication::ArcFlag;
use crate::communication::ArcIdStore;
use crate::communication::IdGenerator;
use crate::communication::MessageReader;
use crate::communication::MessageWriter;
use crate::communication::Poisoner;
use crate::communication::SingleThreadMessageWriter;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::CriticalFormatError;
use crate::plugins::FormatRange;
use crate::plugins::FormatResult;
use crate::plugins::Host;
use crate::plugins::HostFormatRequest;
use crate::plugins::NullCancellationToken;
use crate::plugins::PluginInfo;

#[derive(Debug, Clone)]
pub struct ProcessPluginExecutableInfo {
  pub path: PathBuf,
  pub args: Vec<String>,
}

type DprintCancellationToken = Arc<dyn super::super::CancellationToken>;

enum MessageResponseChannel {
  Acknowledgement(oneshot::Sender<Result<()>>),
  Data(oneshot::Sender<Result<Vec<u8>>>),
  Format(oneshot::Sender<Result<Option<Vec<u8>>>>),
}

#[derive(Clone)]
struct Context {
  stdin_writer: SingleThreadMessageWriter<ProcessPluginMessage>,
  poisoner: Poisoner,
  shutdown_flag: ArcFlag,
  id_generator: IdGenerator,
  messages: ArcIdStore<MessageResponseChannel>,
  format_request_tokens: ArcIdStore<Arc<CancellationToken>>,
  host: Arc<dyn Host>,
}

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
  child: Mutex<Option<Child>>,
  context: Context,
}

impl Drop for ProcessPluginCommunicator {
  fn drop(&mut self) {
    self.kill();
  }
}

impl ProcessPluginCommunicator {
  pub async fn new(
    executable_info: &ProcessPluginExecutableInfo,
    on_std_err: impl Fn(String) + Clone + Send + Sync + 'static,
    host: Arc<dyn Host>,
  ) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_info, false, on_std_err, host).await
  }

  /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
  pub async fn new_with_init(
    executable_info: &ProcessPluginExecutableInfo,
    on_std_err: impl Fn(String) + Clone + Send + Sync + 'static,
    host: Arc<dyn Host>,
  ) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_info, true, on_std_err, host).await
  }

  async fn new_internal(
    executable_info: &ProcessPluginExecutableInfo,
    is_init: bool,
    on_std_err: impl Fn(String) + Clone + Send + Sync + 'static,
    host: Arc<dyn Host>,
  ) -> Result<Self> {
    let mut args = Vec::with_capacity(executable_info.args.len() + 3);
    args.extend(executable_info.args.iter().cloned());
    args.extend(["--parent-pid".to_string(), std::process::id().to_string()]);
    if is_init {
      args.push("--init".to_string());
    }

    let poisoner = Poisoner::default();
    let shutdown_flag = ArcFlag::default();
    let mut child = Command::new(&executable_info.path)
      .args(&args)
      .stdin(Stdio::piped())
      .stderr(Stdio::piped())
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|err| anyhow!("Error starting {} with args [{}]. {:#}", executable_info.path.display(), args.join(" "), err))?;

    // read and output stderr prefixed
    let stderr = child.stderr.take().unwrap();
    tokio::task::spawn_blocking({
      let poisoner = poisoner.clone();
      let shutdown_flag = shutdown_flag.clone();
      let on_std_err = on_std_err.clone();
      move || {
        std_err_redirect(poisoner, shutdown_flag, stderr, on_std_err);
      }
    });

    // verify the schema version
    let mut stdout_reader = MessageReader::new(child.stdout.take().unwrap());
    let mut stdin_writer = MessageWriter::new(child.stdin.take().unwrap());

    verify_plugin_schema_version(&mut stdout_reader, &mut stdin_writer)
      .context("Failed plugin schema verification. This may indicate you are using an old version of the dprint CLI or plugin and should upgrade.")?;

    let stdin_writer = SingleThreadMessageWriter::for_stdin(stdin_writer, poisoner.clone());
    let context = Context {
      id_generator: Default::default(),
      shutdown_flag,
      stdin_writer,
      poisoner,
      messages: Default::default(),
      format_request_tokens: Default::default(),
      host,
    };

    // read from stdout
    tokio::task::spawn_blocking({
      let context = context.clone();
      move || {
        loop {
          if let Err(err) = read_stdout_message(&mut stdout_reader, &context) {
            if !context.poisoner.is_poisoned() && !context.shutdown_flag.is_raised() {
              on_std_err(format!("Error reading stdout message: {:#}", err));
            }
            break;
          }
        }
        context.poisoner.poison();
      }
    });

    Ok(Self {
      child: Mutex::new(Some(child)),
      context,
    })
  }

  /// Perform a graceful shutdown.
  pub async fn shutdown(&self) {
    self.context.shutdown_flag.raise();
    if self.context.poisoner.is_poisoned() {
      self.kill();
    } else {
      // attempt to exit nicely
      tokio::select! {
        // we wait for acknowledgement in order to give the process
        // plugin a chance to clean up (ex. in case it has spawned
        // any processes it needs to kill or something like that)
        _ = self.send_with_acknowledgement(MessageBody::Close) => {}
        _ = tokio::time::sleep(Duration::from_millis(250)) => {}
      }
      self.context.poisoner.poison();
    }
  }

  pub fn kill(&self) {
    self.context.shutdown_flag.raise();
    self.context.poisoner.poison();
    if let Some(mut child) = self.child.lock().take() {
      let _ignore = child.kill();
    }
  }

  pub async fn register_config(&self, config_id: u32, global_config: &GlobalConfiguration, plugin_config: &ConfigKeyMap) -> Result<()> {
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

  pub async fn release_config(&self, config_id: u32) -> Result<()> {
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

  pub async fn resolved_config(&self, config_id: u32) -> Result<String> {
    self.send_receiving_string(MessageBody::GetResolvedConfig(config_id)).await
  }

  pub async fn config_diagnostics(&self, config_id: u32) -> Result<Vec<ConfigurationDiagnostic>> {
    self.send_receiving_data(MessageBody::GetConfigDiagnostics(config_id)).await
  }

  pub async fn format_text(
    &self,
    file_path: PathBuf,
    file_text: String,
    range: FormatRange,
    config_id: u32,
    override_config: ConfigKeyMap,
    token: DprintCancellationToken,
  ) -> FormatResult {
    let (tx, rx) = oneshot::channel::<Result<Option<Vec<u8>>>>();

    let maybe_result = self
      .send_message(
        MessageBody::Format(FormatMessageBody {
          file_path,
          file_text: file_text.into_bytes(),
          range,
          config_id,
          override_config: serde_json::to_vec(&override_config).unwrap(),
        }),
        MessageResponseChannel::Format(tx),
        rx,
        token.clone(),
      )
      .await;

    if token.is_cancelled() {
      Ok(None)
    } else {
      match maybe_result {
        Ok(Ok(Some(bytes))) => Ok(Some(String::from_utf8(bytes)?)),
        Ok(Ok(None)) => Ok(None),
        Ok(Err(err)) => Err(err),
        Err(err) => Err(CriticalFormatError(err).into()),
      }
    }
  }

  /// Checks if the process is functioning.
  pub async fn is_process_alive(&self) -> bool {
    !self.context.poisoner.is_poisoned() || self.ask_is_alive().await
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
    self.context.messages.store(message_id, response_channel);
    self.context.stdin_writer.send(ProcessPluginMessage { id: message_id, body })?;
    tokio::select! {
      _ = self.context.poisoner.wait_poisoned() => {
        self.context.messages.take(message_id); // clear memory
        bail!("Sending message failed because the process plugin failed.");
      }
      _ = token.wait_cancellation() => {
        self.context.messages.take(message_id); // clear memory
        Ok(Ok(Default::default()))
      }
      response = receiver => {
        match response {
          Ok(data) => Ok(data),
          Err(err) => {
            self.context.poisoner.poison();
            bail!("Error waiting on message ({}). {:#}", message_id, err)
          }
        }
      }
    }
  }
}

fn verify_plugin_schema_version<TRead: Read + Unpin, TWrite: Write + Unpin>(
  reader: &mut MessageReader<TRead>,
  writer: &mut MessageWriter<TWrite>,
) -> Result<()> {
  // since this is the setup, use a lot of contexts to find exactly where it failed
  writer.send_u32(0).context("Failed asking for schema version.")?; // ask for schema version
  writer.flush().context("Failed flushing schema version request.")?;
  let acknowledgement_response = reader.read_u32().context("Could not read success response.")?;
  if acknowledgement_response != 0 {
    bail!("Plugin response was unexpected ({acknowledgement_response}).");
  }
  let plugin_schema_version = reader.read_u32().context("Could not read schema version.")?;
  if plugin_schema_version != PLUGIN_SCHEMA_VERSION {
    bail!(
      "The plugin schema version was {}, but expected {}.",
      plugin_schema_version,
      PLUGIN_SCHEMA_VERSION
    );
  }

  Ok(())
}

fn std_err_redirect(poisoner: Poisoner, shutdown_flag: ArcFlag, stderr: ChildStderr, on_std_err: impl Fn(String) + Send + Sync + 'static) {
  use std::io::ErrorKind;
  let reader = std::io::BufReader::new(stderr);
  for line in reader.lines() {
    match line {
      Ok(line) => on_std_err(line),
      Err(err) => {
        if poisoner.is_poisoned() || shutdown_flag.is_raised() {
          return;
        }
        if err.kind() == ErrorKind::BrokenPipe {
          poisoner.poison();
          return;
        } else {
          on_std_err(format!("Error reading line from process plugin stderr. {:#}", err));
        }
      }
    }
  }
}

fn read_stdout_message(reader: &mut MessageReader<ChildStdout>, context: &Context) -> Result<()> {
  let message = ProcessPluginMessage::read(reader)?;

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
    }
    MessageBody::HostFormat(body) => {
      // spawn a task to do the host formatting, then send a message back to the
      // plugin with the result
      let context = context.clone();
      tokio::task::spawn(async move {
        let result = host_format(context.clone(), message.id, body).await;

        // ignore failure, as this means that the process shut down
        // at which point handling would have occurred elsewhere
        let _ignore = context.stdin_writer.send(ProcessPluginMessage {
          id: context.id_generator.next(),
          body: match result {
            Ok(result) => MessageBody::FormatResponse(ResponseBody {
              message_id: message.id,
              data: result.map(|r| r.into_bytes()),
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
    | MessageBody::GetResolvedConfig(_) => {
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

async fn host_format(context: Context, message_id: u32, body: HostFormatMessageBody) -> FormatResult {
  let file_text = String::from_utf8(body.file_text)?; // surface error before storing token

  let token = Arc::new(CancellationToken::new());
  context.format_request_tokens.store(message_id, token.clone());
  let result = context
    .host
    .format(HostFormatRequest {
      file_path: body.file_path,
      file_text,
      range: body.range,
      override_config: serde_json::from_slice(&body.override_config).unwrap(),
      token,
    })
    .await;

  // don't leak cancellation tokens
  context.format_request_tokens.take(message_id);

  result
}
