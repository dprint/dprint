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
use super::messages::Message;
use super::messages::MessageBody;
use super::messages::RegisterConfigMessageBody;
use super::utils::IdGenerator;
use super::utils::Poisoner;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
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
  pub fn new(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, false, on_std_err)
  }

  /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
  pub fn new_with_init(executable_file_path: &Path, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
    ProcessPluginCommunicator::new_internal(executable_file_path, true, on_std_err)
  }

  fn new_internal(executable_file_path: &Path, is_init: bool, on_std_err: impl Fn(String) + Clone + Send + Sync + 'static) -> Result<Self> {
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

  pub async fn register_config(&self, global_config: &GlobalConfiguration, plugin_config: &ConfigKeyMap) -> Result<u32> {
    let config_id = self.context.id_generator.next();
    let global_config = serde_json::to_vec(global_config)?;
    let plugin_config = serde_json::to_vec(plugin_config)?;
    self
      .send_with_acknowledgement(MessageBody::RegisterConfig(RegisterConfigMessageBody {
        config_id,
        global_config,
        plugin_config,
      }))
      .await?;
    Ok(config_id)
  }

  pub async fn get_plugin_info(&self) -> Result<PluginInfo> {
    self.send_receiving_data(MessageBody::GetPluginInfo).await
  }

  pub async fn get_license_text(&self) -> Result<String> {
    self.send_receiving_string(MessageBody::GetLicenseText).await
  }

  pub async fn get_resolved_config(&self, config_id: u32) -> Result<String> {
    self.send_receiving_string(MessageBody::GetResolvedConfig(config_id)).await
  }

  pub async fn get_config_diagnostics(&self, config_id: u32) -> Result<Vec<ConfigurationDiagnostic>> {
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
  /// Only use this after an error has occurred to tell if the process should be recreated.
  pub async fn is_process_alive(&mut self) -> bool {
    self.context.poisoner.is_poisoned() || self.get_plugin_info().await.is_ok()
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
  // todo: don't unwrap here. Instead find out when an error occurs
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
        reader.read_success_bytes();
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
      todo!();
    }
    _ => {
      bail!("Unknown response kind: {}", kind);
    }
  }

  Ok(())
}
