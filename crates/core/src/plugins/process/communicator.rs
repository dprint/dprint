use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStderr;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use super::communication::MessageReader;
use super::communication::MessageWriter;
use super::messages::FormatTextMessageBody;
use super::messages::HostFormatResponseMessageBody;
use super::messages::Message;
use super::messages::MessageBody;
use super::messages::RegisterConfigMessageBody;
use super::messages::ResponseBodyHostFormat;
use super::utils::IdGenerator;
use super::utils::Poisoner;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::Host;
use crate::plugins::PluginInfo;

enum MessageResponseChannel {
  Acknowledgement(oneshot::Sender<Result<()>>),
  Data(oneshot::Sender<Result<Vec<u8>>>),
  Format(oneshot::Sender<Result<Option<Vec<u8>>>>),
}

#[derive(Clone, Default)]
struct MessageResponses(Arc<Mutex<HashMap<u32, MessageResponseChannel>>>);

impl MessageResponses {
  pub fn store(&self, message_id: u32, response: MessageResponseChannel) {
    self.0.lock().insert(message_id, response);
  }

  pub fn take(&self, message_id: u32) -> Result<MessageResponseChannel> {
    match self.0.lock().remove(&message_id) {
      Some(value) => Ok(value),
      None => bail!("Could not find message with id: {}", message_id),
    }
  }
}

#[derive(Clone)]
struct Context {
  message_tx: UnboundedSender<Message>,
  poisoner: Poisoner,
  id_generator: IdGenerator,
  messages: MessageResponses,
  host: Arc<dyn Host>,
}

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
  child: Child,
  context: Context,
}

impl Drop for ProcessPluginCommunicator {
  fn drop(&mut self) {
    let _ignore = self.close();
    self.context.poisoner.poison();
  }
}

impl ProcessPluginCommunicator {
  pub fn new(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static, host: Arc<dyn Host>) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, false, on_std_err, host)
  }

  /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
  pub fn new_with_init(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static, host: Arc<dyn Host>) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, true, on_std_err, host)
  }

  fn new_internal(
    executable_file_path: &Path,
    is_init: bool,
    on_std_err: impl Fn(String) + Clone + Send + Sync + 'static,
    host: Arc<dyn Host>,
  ) -> Result<Self> {
    let mut args = vec!["--parent-pid".to_string(), std::process::id().to_string()];
    if is_init {
      args.push("--init".to_string());
    }

    let poisoner = Poisoner::default();
    let mut child = Command::new(executable_file_path)
      .args(&args)
      .stdin(Stdio::piped())
      .stderr(Stdio::piped())
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|err| anyhow!("Error starting {} with args [{}]. {}", executable_file_path.display(), args.join(" "), err))?;

    // read and output stderr prefixed
    let stderr = child.stderr.take().unwrap();
    tokio::task::spawn_blocking({
      let poisoner = poisoner.clone();
      let on_std_err = on_std_err.clone();
      move || {
        std_err_redirect(poisoner, stderr, on_std_err);
      }
    });

    // verify the schema version
    let mut reader = MessageReader::new(child.stdout.take().unwrap());
    let mut writer = MessageWriter::new(child.stdin.take().unwrap());
    verify_plugin_schema_version(&mut reader, &mut writer)?;

    let (message_tx, mut message_rx) = unbounded_channel::<Message>();
    let context = Context {
      id_generator: Default::default(),
      message_tx,
      poisoner: poisoner.clone(),
      messages: Default::default(),
      host,
    };

    // read from stdout
    tokio::task::spawn_blocking({
      let context = context.clone();
      move || loop {
        if let Err(err) = read_stdout_message(&mut reader, &context) {
          if !context.poisoner.is_poisoned() {
            on_std_err(format!("Error reading stdout message. {}", err));
            context.poisoner.poison();
          }
          break;
        }
      }
    });

    tokio::task::spawn({
      let poisoner = poisoner.clone();
      async move {
        while let Some(message) = message_rx.recv().await {
          if let Err(_) = message.write(&mut writer) {
            break;
          }
          if matches!(message.body, MessageBody::Close) {
            break;
          }
        }
        poisoner.poison();
      }
    });

    Ok(Self { child, context })
  }

  fn close(&mut self) -> Result<()> {
    if self.context.poisoner.is_poisoned() {
      self.kill();
    } else {
      // attempt to exit nicely
      let result = self.context.message_tx.send(Message {
        id: self.context.id_generator.next(),
        body: MessageBody::Close,
      });

      // otherwise, ensure kill
      if let Err(_) = result {
        self.kill();
      }
    }

    Ok(())
  }

  pub fn kill(&mut self) {
    let _ignore = self.child.kill();
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

  pub async fn is_alive(&self) -> bool {
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
    config_id: u32,
    override_config: Option<&ConfigKeyMap>,
    range: Option<Range<usize>>,
  ) -> Result<Option<String>> {
    let (tx, rx) = oneshot::channel::<Result<Option<Vec<u8>>>>();
    let maybe_text = self
      .send_message(
        MessageBody::FormatText(FormatTextMessageBody {
          file_path,
          file_text: file_text.into_bytes(),
          config_id,
          override_config: override_config.map(|c| serde_json::to_vec(c).unwrap()),
          range,
        }),
        MessageResponseChannel::Format(tx),
        rx,
      )
      .await?;
    match maybe_text {
      Some(bytes) => Ok(Some(String::from_utf8(bytes)?)),
      None => Ok(None),
    }
  }

  /// Checks if the process is functioning.
  pub async fn is_process_alive(&self) -> bool {
    self.context.poisoner.is_poisoned() || self.is_alive().await
  }

  async fn send_with_acknowledgement(&self, body: MessageBody) -> Result<()> {
    let (tx, rx) = oneshot::channel::<Result<()>>();
    self.send_message(body, MessageResponseChannel::Acknowledgement(tx), rx).await
  }

  async fn send_receiving_string(&self, body: MessageBody) -> Result<String> {
    let data = self.send_receiving_bytes(body).await?;
    Ok(String::from_utf8(data)?)
  }

  async fn send_receiving_data<T: DeserializeOwned>(&self, body: MessageBody) -> Result<T> {
    let data = self.send_receiving_bytes(body).await?;
    Ok(serde_json::from_slice(&data)?)
  }

  async fn send_receiving_bytes(&self, body: MessageBody) -> Result<Vec<u8>> {
    let (tx, rx) = oneshot::channel::<Result<Vec<u8>>>();
    self.send_message(body, MessageResponseChannel::Data(tx), rx).await
  }

  async fn send_message<T>(&self, body: MessageBody, response_channel: MessageResponseChannel, receiver: oneshot::Receiver<Result<T>>) -> Result<T> {
    let message_id = self.context.id_generator.next();
    self.context.messages.store(message_id, response_channel);
    self.context.message_tx.send(Message { id: message_id, body })?;
    match receiver.await {
      Ok(data) => data,
      Err(err) => {
        self.context.poisoner.poison();
        bail!("Error waiting on message ({}). {}", message_id, err)
      }
    }
  }
}

fn verify_plugin_schema_version<TRead: Read, TWrite: Write>(reader: &mut MessageReader<TRead>, writer: &mut MessageWriter<TWrite>) -> Result<()> {
  // do this synchronously at the start
  writer.send_u32(0)?; // ask for schema version
  if reader.read_u32()? != 0 {
    bail!(concat!(
      "There was a problem checking the plugin schema version. ",
      "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade."
    ));
  }
  let plugin_schema_version = reader.read_u32()?;
  if plugin_schema_version != PLUGIN_SCHEMA_VERSION {
    bail!(
      concat!(
        "The plugin schema version was {}, but expected {}. ",
        "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade."
      ),
      plugin_schema_version,
      PLUGIN_SCHEMA_VERSION
    );
  }

  Ok(())
}

fn std_err_redirect(poisoner: Poisoner, stderr: ChildStderr, on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static) {
  use std::io::BufRead;
  use std::io::ErrorKind;
  let reader = std::io::BufReader::new(stderr);
  for line in reader.lines() {
    match line {
      Ok(line) => on_std_err(line),
      Err(err) => {
        if poisoner.is_poisoned() {
          return;
        }
        if err.kind() == ErrorKind::BrokenPipe {
          poisoner.poison();
          return;
        } else {
          on_std_err(format!("Error reading line from process plugin stderr. {}", err));
        }
      }
    }
  }
}

fn read_stdout_message(reader: &mut MessageReader<ChildStdout>, context: &Context) -> Result<()> {
  let id = reader.read_u32()?;
  let message = context.messages.take(id)?;
  let kind = reader.read_u32()?;
  match kind {
    // Success
    0 => match message {
      MessageResponseChannel::Acknowledgement(channel) => {
        reader.read_success_bytes()?;
        let _ignore = channel.send(Ok(()));
      }
      MessageResponseChannel::Data(channel) => {
        let bytes = reader.read_sized_bytes()?;
        reader.read_success_bytes()?;
        let _ignore = channel.send(Ok(bytes));
      }
      MessageResponseChannel::Format(channel) => {
        let response_kind = reader.read_u32()?;
        let data = match response_kind {
          0 => None,
          1 => Some(reader.read_sized_bytes()?),
          _ => bail!("Invalid format response kind: {}", response_kind),
        };

        let _ignore = channel.send(Ok(data));
      }
    },
    // Error
    1 => {
      let bytes = reader.read_sized_bytes()?;
      reader.read_success_bytes()?;
      let err = anyhow!("{}", String::from_utf8_lossy(&bytes));
      match message {
        MessageResponseChannel::Acknowledgement(channel) => {
          let _ignore = channel.send(Err(err));
        }
        MessageResponseChannel::Data(channel) => {
          let _ignore = channel.send(Err(err));
        }
        MessageResponseChannel::Format(channel) => {
          let _ignore = channel.send(Err(err));
        }
      }
    }
    // Host format
    2 => {
      let file_path = reader.read_sized_bytes()?;
      let start_byte_index = reader.read_u32()?;
      let end_byte_index = reader.read_u32()?;
      let override_config = reader.read_sized_bytes()?;
      let file_text = reader.read_sized_bytes()?;
      reader.read_success_bytes()?;
      let body = ResponseBodyHostFormat {
        file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
        range: if start_byte_index == 0 && end_byte_index as usize == file_text.len() {
          None
        } else {
          Some(Range {
            start: start_byte_index as usize,
            end: end_byte_index as usize,
          })
        },
        file_text,
        override_config: if override_config.is_empty() { None } else { Some(override_config) },
      };

      // spawn a task to do the host formatting, then send a message back to the
      // plugin with the result
      let context = context.clone();
      tokio::task::spawn(async move {
        let result = host_format(context.host.clone(), body).await;
        // ignore failure, as this means that the process shut down
        // at which point handling would have occurred elsewhere
        let _ignore = context.message_tx.send(Message {
          id,
          body: MessageBody::HostFormatResponse(match result {
            Ok(Some(text)) => HostFormatResponseMessageBody::Change(text.into_bytes()),
            Ok(None) => HostFormatResponseMessageBody::NoChange,
            Err(err) => HostFormatResponseMessageBody::Error(format!("{}", err).into_bytes()),
          }),
        });
      });
    }
    _ => {
      bail!("Unknown response kind: {}", kind);
    }
  }

  Ok(())
}

async fn host_format(host: Arc<dyn Host>, body: ResponseBodyHostFormat) -> Result<Option<String>> {
  host
    .format(
      body.file_path,
      String::from_utf8(body.file_text)?,
      body.range,
      body.override_config.map(|c| serde_json::from_slice(&c).unwrap()),
    )
    .await
}
