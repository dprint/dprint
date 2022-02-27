use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use std::borrow::Cow;
use std::future::Future;
use std::io::Read;
use std::io::Stdin;
use std::io::Stdout;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::communication::StdinReader;
use super::communication::StdoutWriter;
use super::context::ProcessContext;
use super::FormatResult;
use super::HostFormatResult;
use super::MessageKind;
use super::ResponseKind;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::FormatRequest;
use crate::plugins::Host;
use crate::plugins::PluginHandler;

struct Response {
  id: u32,
  body: ResponseBody,
}

enum ResponseBody {
  Success(ResponseSuccessBody),
  Error(String),
  HostFormat(ResponseBodyHostFormat),
}

struct ResponseBodyHostFormat {
  file_path: PathBuf,
  range: Range<u32>,
  override_config: Vec<u8>,
  file_text: String,
}

enum ResponseSuccessBody {
  General,
  Data(Vec<u8>),
  FormatText(Option<String>),
}

struct MessageProcessorState<TConfiguration: Clone + Serialize> {
  global_config: Option<GlobalConfiguration>,
  config: Option<ConfigKeyMap>,
  resolved_config_result: Option<ResolveConfigurationResult<TConfiguration>>,
}

/// Handles the process' messages based on the provided handler.
pub fn handle_process_stdio_messages<THandler: PluginHandler>(handler: THandler) -> Result<()> {
  // ensure all process plugins exit on panic on any tokio task
  setup_exit_process_panic_hook();

  let mut stdin = std::io::stdin();
  let mut stdout = std::io::stdout();

  schema_establishment_phase(&mut stdin, &mut stdout)?;

  let handler = Arc::new(handler);
  let context: ProcessContext<THandler::Configuration> = ProcessContext::new();
  let (response_tx, mut response_rx) = mpsc::unbounded_channel::<Response>();
  let host = ProcessHost {
    context: context.clone(),
    sender: response_tx.clone(),
  };

  // task to send responses
  tokio::task::spawn({
    let handler = handler.clone();
    let context = context.clone();
    async move {
      let stdout_writer = StdoutWriter::new(stdout);
      while let Some(result) = response_rx.recv().await {
        stdout_writer.send_u32(result.id);
        match result.body {
          ResponseBody::Success(body) => {
            stdout_writer.send_u32(ResponseKind::Success as u32);
            match body {
              ResponseSuccessBody::General => {
                // do nothing, success bytes will be sent
              }
              ResponseSuccessBody::Data(data) => {
                stdout_writer.send_sized_bytes(&data);
              }
              ResponseSuccessBody::FormatText(maybe_text) => match maybe_text {
                Some(text) => {
                  stdout_writer.send_u32(FormatResult::Change as u32);
                  stdout_writer.send_sized_bytes(text.as_bytes());
                }
                None => {
                  stdout_writer.send_u32(FormatResult::NoChange as u32);
                }
              },
            }
          }
          ResponseBody::Error(text) => {
            stdout_writer.send_u32(ResponseKind::Error as u32);
            stdout_writer.send_sized_bytes(&text.as_bytes());
          }
          ResponseBody::HostFormat(data) => {
            stdout_writer.send_u32(ResponseKind::HostFormatRequest as u32);
          }
        }
        stdout_writer.send_success_bytes();
      }
    }
  });

  // task to read stdin messages
  let mut stdin_reader = StdinReader::new(stdin);
  loop {
    let id = stdin_reader.read_u32();
    let kind: MessageKind = stdin_reader.read_u32().into();
    match kind {
      MessageKind::Close => {
        return Ok(());
      }
      MessageKind::GetPluginInfo => {
        stdin_reader.read_success_bytes();
        handle_message(&response_tx, id, || {
          let plugin_info = handler.plugin_info();
          let data = serde_json::to_vec(&plugin_info)?;
          Ok(ResponseSuccessBody::Data(data))
        });
      }
      MessageKind::GetLicenseText => {
        stdin_reader.read_success_bytes();
        handle_message(&response_tx, id, || Ok(ResponseSuccessBody::Data(handler.license_text().into_bytes())));
      }
      MessageKind::RegisterConfig => {
        // read bytes first
        let global_config = stdin_reader.read_sized_bytes();
        let plugin_config = stdin_reader.read_sized_bytes();
        stdin_reader.read_success_bytes();

        handle_message(&response_tx, id, || {
          let global_config = serde_json::from_slice(&global_config)?;
          let plugin_config = serde_json::from_slice(&plugin_config)?;
          let result = handler.resolve_config(&global_config, plugin_config);
          context.store_config_result(id, result);
          Ok(ResponseSuccessBody::General)
        });
      }
      MessageKind::ReleaseConfig => {
        stdin_reader.read_success_bytes();
        handle_message(&response_tx, id, || {
          context.release_config_result(id);
          Ok(ResponseSuccessBody::General)
        });
      }
      MessageKind::GetConfigDiagnostics => {
        stdin_reader.read_success_bytes();
        handle_message(&response_tx, id, || {
          let result = serde_json::to_vec(&context.get_config_diagnostics(id))?;
          Ok(ResponseSuccessBody::Data(result))
        });
      }
      MessageKind::GetResolvedConfig => {
        stdin_reader.read_success_bytes();
        handle_message(&response_tx, id, || {
          let result = match context.get_config(id) {
            Some(config) => serde_json::to_vec(&config)?,
            None => bail!("Did not find configuration for id: {}", id),
          };
          Ok(ResponseSuccessBody::Data(result))
        });
      }
      MessageKind::FormatText => {
        // read bytes first
        let file_path = stdin_reader.read_sized_bytes();
        let start_byte_index = stdin_reader.read_u32();
        let end_byte_index = stdin_reader.read_u32();
        let config_id = stdin_reader.read_u32();
        let json_override_config = stdin_reader.read_sized_bytes();
        let file_text = stdin_reader.read_sized_bytes();
        stdin_reader.read_success_bytes();

        // now parse
        let token = CancellationToken::new();
        let request = FormatRequest {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          range: if start_byte_index == 0 && end_byte_index == file_text.len() as u32 {
            None
          } else {
            Some(Range {
              start: start_byte_index as usize,
              end: end_byte_index as usize,
            })
          },
          config: match context.get_config(config_id) {
            Some(config) => config,
            None => {
              send_response(
                &response_tx,
                Response {
                  id,
                  body: ResponseBody::Error(format!("Did not find configuration for id: {}", config_id)),
                },
              );
              continue;
            }
          },
          file_text: match String::from_utf8(file_text) {
            Ok(text) => text,
            Err(err) => {
              send_response(
                &response_tx,
                Response {
                  id,
                  body: ResponseBody::Error(format!("Error decoding text: {}", err)),
                },
              );
              continue;
            }
          },
          token: token.clone(),
        };

        // start the task
        context.store_cancellation_token(id, token.clone());
        let context = context.clone();
        let handler = handler.clone();
        let host = host.clone();
        let response_tx = response_tx.clone();
        tokio::task::spawn(async move {
          let result = handler.format(request, host).await;
          context.release_cancellation_token(id);
          if !token.is_cancelled() {
            handle_message(&response_tx, id, || Ok(ResponseSuccessBody::FormatText(result?)));
          }
        });
      }
      MessageKind::CancelFormat => {
        stdin_reader.read_success_bytes();
        context.cancel_format(id);
      }
      MessageKind::HostFormatResponse => {
        let response_kind: HostFormatResult = stdin_reader.read_u32().into();
        let data = match response_kind {
          HostFormatResult::NoChange => None,
          HostFormatResult::Change => Some(stdin_reader.read_sized_bytes()),
        };
        stdin_reader.read_success_bytes();
        if let Some(sender) = context.take_format_host_sender(id) {
          let data = data.map(|text| String::from_utf8(text).ok()).flatten();
          sender.send
        }
      }
    }
  }
}

#[derive(Clone)]
struct ProcessHost<TConfiguration: Serialize + Clone> {
  context: ProcessContext<TConfiguration>,
  sender: UnboundedSender<Response>,
}

impl<TConfiguration: Serialize + Clone> Host for ProcessHost<TConfiguration> {
  type FormatFuture = Pin<Box<dyn Future<Output = Result<Option<String>>>>>;

  fn format(&self, file_path: PathBuf, file_text: String, range: Option<Range<usize>>, config: &ConfigKeyMap) -> Self::FormatFuture {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Option<String>>>();
    let id = self.context.store_format_host_sender(tx);

    self
      .sender
      .send(Response {
        id,
        body: ResponseBody::HostFormat(ResponseBodyHostFormat {
          range: Range {
            start: range.map(|r| r.start as u32).unwrap_or(0),
            end: range.map(|r| r.end as u32).unwrap_or(file_text.len() as u32),
          },
          file_path,
          file_text,
          override_config: serde_json::to_vec(config).unwrap(),
        }),
      })
      .unwrap_or_else(|err| panic!("Error sending host format response: {}", err));

    Box::pin(async move {
      match rx.await {
        Ok(value) => value,
        Err(err) => Err(anyhow!("{}", err)),
      }
    })
  }
}

impl crate::plugins::CancellationToken for CancellationToken {
  fn is_cancelled(&self) -> bool {
    self.is_cancelled()
  }
}

fn handle_message(response_tx: &mpsc::UnboundedSender<Response>, id: u32, action: impl Fn() -> Result<ResponseSuccessBody>) {
  let body = match action() {
    Ok(response) => ResponseBody::Success(response),
    Err(err) => ResponseBody::Error(err.to_string()),
  };
  send_response(response_tx, Response { id, body });
}

fn send_response(response_tx: &mpsc::UnboundedSender<Response>, response: Response) {
  if let Err(err) = response_tx.send(response) {
    panic!("Receiver dropped. {}", err);
  }
}

/// For backwards compatibility asking for the schema version.
fn schema_establishment_phase(stdin: &mut Stdin, stdout: &mut Stdout) -> Result<()> {
  // 1. An initial `0` (4 bytes) is sent asking for the schema version.
  let mut receive_buf: [u8; 4] = [0; 4];
  stdin.read_exact(&mut receive_buf)?;
  let value = u32::from_be_bytes(receive_buf);
  if value != 0 {
    bail!("Expected a schema version request of `0`.");
  }

  // 2. The client responds with `0` (4 bytes) for success, then `4` (4 bytes) for the schema version.
  let mut response_buf: [u8; 8] = [0; 8];
  stdout.write(&(0 as u32).to_be_bytes());
  stdout.write(&PLUGIN_SCHEMA_VERSION.to_be_bytes());

  Ok(())
}

fn handle_message_kind<TRead: Read, TWrite: Write, THandler: PluginHandler>(
  message_kind: MessageKind,
  messenger: &mut StdIoMessenger<TRead, TWrite>,
  handler: &mut THandler,
  state: &mut MessageProcessorState<THandler::Configuration>,
) -> Result<bool> {
  match message_kind {
    MessageKind::Close => {
      messenger.read_zero_part_message()?;
      return Ok(false);
    }
    MessageKind::GetPluginSchemaVersion => {
      messenger.read_zero_part_message()?;
      messenger.send_response(vec![PLUGIN_SCHEMA_VERSION.into()])?
    }
    MessageKind::GetPluginInfo => {
      messenger.read_zero_part_message()?;
      messenger.send_response(vec![serde_json::to_vec(&handler.plugin_info())?.into()])?
    }
    MessageKind::GetLicenseText => {
      messenger.read_zero_part_message()?;
      messenger.send_response(vec![handler.license_text().into()])?
    }
    MessageKind::SetGlobalConfig => {
      let message_data = messenger.read_single_part_message()?;
      state.global_config = Some(serde_json::from_slice(&message_data)?);
      state.resolved_config_result.take();
      messenger.send_response(Vec::new())?;
    }
    MessageKind::SetPluginConfig => {
      let message_data = messenger.read_single_part_message()?;
      let plugin_config = serde_json::from_slice(&message_data)?;
      state.resolved_config_result.take();
      state.config = Some(plugin_config);
      messenger.send_response(Vec::new())?;
    }
    MessageKind::GetResolvedConfig => {
      messenger.read_zero_part_message()?;
      ensure_resolved_config(handler, state)?;
      let resolved_config = get_resolved_config_result(state)?;
      messenger.send_response(vec![serde_json::to_vec(&resolved_config.config)?.into()])?
    }
    MessageKind::GetConfigDiagnostics => {
      messenger.read_zero_part_message()?;
      ensure_resolved_config(handler, state)?;
      let resolved_config = get_resolved_config_result(state)?;
      messenger.send_response(vec![serde_json::to_vec(&resolved_config.diagnostics)?.into()])?
    }
    MessageKind::FormatText => {
      let mut parts = messenger.read_multi_part_message(3)?;
      ensure_resolved_config(handler, state)?;
      let file_path = parts.take_path_buf()?;
      let file_text = parts.take_string()?;
      let override_config: ConfigKeyMap = serde_json::from_slice(&parts.take_part()?)?;
      let config = if !override_config.is_empty() {
        Cow::Owned(create_resolved_config_result(handler, state, override_config)?.config)
      } else {
        Cow::Borrowed(&get_resolved_config_result(state)?.config)
      };

      let formatted_text = handler.format(&file_path, &file_text, &config, |file_path, file_text, override_config| {
        format_with_host(messenger, file_path, file_text, override_config)
      })?;

      if formatted_text == file_text {
        messenger.send_response(vec![(FormatResult::NoChange as u32).into()])?;
      } else {
        messenger.send_response(vec![(FormatResult::Change as u32).into(), formatted_text.into()])?;
      }
    }
  }

  Ok(true)
}

fn ensure_resolved_config<THandler: PluginHandler>(handler: &mut THandler, state: &mut MessageProcessorState<THandler::Configuration>) -> Result<()> {
  if state.resolved_config_result.is_none() {
    state.resolved_config_result = Some(create_resolved_config_result(handler, state, Default::default())?);
  }

  Ok(())
}

fn create_resolved_config_result<THandler: PluginHandler>(
  handler: &mut THandler,
  state: &MessageProcessorState<THandler::Configuration>,
  override_config: ConfigKeyMap,
) -> Result<ResolveConfigurationResult<THandler::Configuration>> {
  let mut plugin_config = state
    .config
    .as_ref()
    .ok_or_else(|| anyhow!("Expected plugin config to be set at this point"))?
    .clone();
  for (key, value) in override_config {
    plugin_config.insert(key, value);
  }
  Ok(
    handler.resolve_config(
      plugin_config,
      state
        .global_config
        .as_ref()
        .ok_or_else(|| anyhow!("Expected global config to be set at this point."))?,
    ),
  )
}

fn get_resolved_config_result<TConfiguration: Clone + Serialize>(
  state: &MessageProcessorState<TConfiguration>,
) -> Result<&ResolveConfigurationResult<TConfiguration>> {
  state
    .resolved_config_result
    .as_ref()
    .ok_or_else(|| anyhow!("Expected the config to be resolved at this point."))
}

fn format_with_host<TRead: Read, TWrite: Write>(
  messenger: &mut StdIoMessenger<TRead, TWrite>,
  file_path: &Path,
  file_text: String,
  override_config: &ConfigKeyMap,
) -> Result<String> {
  messenger.send_response(vec![
    (FormatResult::RequestTextFormat as u32).into(),
    file_path.into(),
    file_text.as_str().into(),
    (&serde_json::to_vec(&override_config)?).into(),
  ])?;

  let format_result = messenger.read_code()?.into();
  match format_result {
    HostFormatResult::Change => messenger.read_single_part_string_message(),
    HostFormatResult::NoChange => {
      messenger.read_zero_part_message()?; // ensures success is read
      Ok(file_text)
    }
    HostFormatResult::Error => {
      bail!("{}", messenger.read_single_part_error_message()?)
    }
  }
}

trait StdIoMessengerExtensions {
  fn send_response(&mut self, message_parts: Vec<MessagePart>) -> Result<()>;
  fn send_error_response(&mut self, error_message: &str) -> Result<()>;
}

impl<TRead: Read, TWrite: Write> StdIoMessengerExtensions for StdIoMessenger<TRead, TWrite> {
  fn send_response(&mut self, message_parts: Vec<MessagePart>) -> Result<()> {
    self.send_message(ResponseKind::Success as u32, message_parts)
  }

  fn send_error_response(&mut self, error_message: &str) -> Result<()> {
    self.send_message(ResponseKind::Error as u32, vec![error_message.into()])
  }
}

fn setup_exit_process_panic_hook() {
  // tokio doesn't exit on task panic, so implement that behaviour here
  let orig_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    orig_hook(panic_info);
    std::process::exit(1);
  }));
}
