use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::communication::MessageReader;
use super::communication::MessageWriter;
use super::context::ProcessContext;
use super::context::StoredConfig;
use super::messages::HostFormatMessageBody;
use super::messages::Message;
use super::messages::MessageBody;
use super::messages::ResponseBody;
use super::utils::setup_exit_process_panic_hook;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::plugins::AsyncPluginHandler;
use crate::plugins::FormatRequest;
use crate::plugins::FormatResult;
use crate::plugins::Host;
use crate::plugins::HostFormatRequest;

/// Handles the process' messages based on the provided handler.
pub async fn handle_process_stdio_messages<THandler: AsyncPluginHandler>(handler: THandler) -> Result<()> {
  // ensure all process plugins exit on panic on any tokio task
  setup_exit_process_panic_hook();

  let mut stdin_reader = MessageReader::new(tokio::io::stdin());
  let mut stdout_writer = MessageWriter::new(tokio::io::stdout());

  schema_establishment_phase(&mut stdin_reader, &mut stdout_writer).await?;

  let handler = Arc::new(handler);
  let (response_tx, mut response_rx) = mpsc::unbounded_channel::<Message>();
  let context: ProcessContext<THandler::Configuration> = ProcessContext::new(response_tx);
  let host = Arc::new(ProcessHost { context: context.clone() });

  // task to send responses over stdout
  tokio::task::spawn({
    async move {
      while let Some(result) = response_rx.recv().await {
        result.write(&mut stdout_writer).await.unwrap();
      }
    }
  });

  // read messages over stdin
  loop {
    let message = Message::read(&mut stdin_reader).await?;

    match message.body {
      MessageBody::Close => {
        return Ok(());
      }
      MessageBody::IsAlive => {
        handle_message(&context, message.id, || Ok(MessageBody::Success(message.id)));
      }
      MessageBody::GetPluginInfo => {
        handle_message(&context, message.id, || {
          let plugin_info = handler.plugin_info();
          let data = serde_json::to_vec(&plugin_info)?;
          Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
        });
      }
      MessageBody::GetLicenseText => {
        handle_message(&context, message.id, || {
          let data = handler.license_text().into_bytes();
          Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
        });
      }
      MessageBody::RegisterConfig(body) => {
        handle_message(&context, message.id, || {
          let global_config: GlobalConfiguration = serde_json::from_slice(&body.global_config)?;
          let config_map: ConfigKeyMap = serde_json::from_slice(&body.plugin_config)?;
          let result = handler.resolve_config(config_map.clone(), global_config.clone());
          context.store_config_result(
            body.config_id,
            StoredConfig {
              config: Arc::new(result.config),
              diagnostics: Arc::new(result.diagnostics),
              config_map,
              global_config,
            },
          );
          Ok(MessageBody::Success(message.id))
        });
      }
      MessageBody::ReleaseConfig(config_id) => {
        handle_message(&context, message.id, || {
          context.release_config_result(config_id);
          Ok(MessageBody::Success(message.id))
        });
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        handle_message(&context, message.id, || {
          let diagnostics = context.get_config(config_id).map(|c| c.diagnostics.clone()).unwrap_or_default();
          let data = serde_json::to_vec(&*diagnostics)?;
          Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
        });
      }
      MessageBody::GetResolvedConfig(config_id) => {
        handle_message(&context, message.id, || {
          let data = match context.get_config(config_id) {
            Some(config) => serde_json::to_vec(&*config.config)?,
            None => bail!("Did not find configuration for id: {}", config_id),
          };
          Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
        });
      }
      MessageBody::Format(body) => {
        // now parse
        let token = Arc::new(CancellationToken::new());
        let request = FormatRequest {
          file_path: body.file_path,
          range: body.range,
          config: match context.get_config(body.config_id) {
            Some(config) => {
              if body.override_config.is_empty() {
                config.config.clone()
              } else {
                let mut config_map = config.config_map.clone();
                let override_config_map: ConfigKeyMap = serde_json::from_slice(&body.override_config)?;
                for (key, value) in override_config_map {
                  config_map.insert(key, value);
                }
                let result = handler.resolve_config(config_map, config.global_config.clone());
                Arc::new(result.config)
              }
            }
            None => {
              send_error_response(&context, message.id, anyhow!("Did not find configuration for id: {}", body.config_id));
              continue;
            }
          },
          file_text: match String::from_utf8(body.file_text) {
            Ok(text) => text,
            Err(err) => {
              send_error_response(&context, message.id, anyhow!("Error decoding text to utf8: {}", err));
              continue;
            }
          },
          token: token.clone(),
        };

        // start the task
        context.store_cancellation_token(message.id, token.clone());
        let context = context.clone();
        let handler = handler.clone();
        let host = host.clone();
        tokio::task::spawn(async move {
          let result = handler.format(request, host).await;
          context.release_cancellation_token(message.id);
          if !token.is_cancelled() {
            let body = match result {
              Ok(text) => MessageBody::FormatResponse(ResponseBody {
                message_id: message.id,
                data: text.map(|t| t.into_bytes()),
              }),
              Err(err) => MessageBody::Error(ResponseBody {
                message_id: message.id,
                data: format!("{}", err).into_bytes(),
              }),
            };
            send_response_body(&context, body)
          }
        });
      }
      MessageBody::CancelFormat(message_id) => {
        context.cancel_format(message_id);
      }
      MessageBody::Error(body) => {
        let text = String::from_utf8_lossy(&body.data);
        if let Some(sender) = context.take_format_host_sender(body.message_id) {
          sender.send(Err(anyhow!("{}", text))).unwrap();
        } else {
          eprintln!("Received error from CLI. {}", text);
        }
      }
      MessageBody::FormatResponse(body) => {
        let data = match body.data {
          None => Ok(None),
          Some(data) => match String::from_utf8(data) {
            Ok(data) => Ok(Some(data)),
            Err(err) => Err(anyhow!("Error deserializing success: {}", err)),
          },
        };
        if let Some(sender) = context.take_format_host_sender(body.message_id) {
          sender.send(data).unwrap();
        }
      }
      MessageBody::Success(_) | MessageBody::DataResponse(_) => {
        // ignore
      }
      MessageBody::HostFormat(_) => {
        send_error_response(&context, message.id, anyhow!("Cannot host format with a plugin."));
      }
      MessageBody::Unknown(message_kind) => panic!("Received unknown message kind: {}", message_kind),
    }
  }
}

struct ProcessHost<TConfiguration: Serialize + Clone + Send + Sync> {
  context: ProcessContext<TConfiguration>,
}

impl<TConfiguration: Serialize + Clone + Send + Sync> Host for ProcessHost<TConfiguration> {
  fn format(&self, request: HostFormatRequest) -> Pin<Box<dyn Future<Output = FormatResult> + Send>> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Option<String>>>();
    let id = self.context.id_generator.next();
    self.context.store_format_host_sender(id, tx);

    // todo: start a task that listens for cancellation

    self
      .context
      .response_tx
      .send(Message {
        id,
        body: MessageBody::HostFormat(HostFormatMessageBody {
          file_path: request.file_path,
          file_text: request.file_text.into_bytes(),
          range: request.range,
          override_config: serde_json::to_vec(&request.override_config).unwrap(),
        }),
      })
      .unwrap_or_else(|err| panic!("Error sending host format response: {}", err));

    Box::pin(async move {
      match rx.await {
        Ok(Ok(Some(value))) => Ok(Some(value)),
        Ok(Ok(None)) => Ok(None),
        Ok(Err(err)) => Err(err),
        // means the rx was closed, so just ignore
        Err(err) => Err(err.into()),
      }
    })
  }
}

impl crate::plugins::CancellationToken for CancellationToken {
  fn is_cancelled(&self) -> bool {
    self.is_cancelled()
  }
}

fn handle_message<TConfiguration: Serialize + Clone + Send + Sync>(
  context: &ProcessContext<TConfiguration>,
  original_message_id: u32,
  action: impl FnOnce() -> Result<MessageBody>,
) {
  match action() {
    Ok(body) => send_response_body(context, body),
    Err(err) => send_error_response(context, original_message_id, err),
  };
}

fn send_error_response<TConfiguration: Serialize + Clone + Send + Sync>(
  context: &ProcessContext<TConfiguration>,
  original_message_id: u32,
  err: anyhow::Error,
) {
  let body = MessageBody::Error(ResponseBody {
    message_id: original_message_id,
    data: format!("{}", err).into_bytes(),
  });
  send_response_body(context, body)
}

fn send_response_body<TConfiguration: Serialize + Clone + Send + Sync>(context: &ProcessContext<TConfiguration>, body: MessageBody) {
  let message = Message {
    id: context.id_generator.next(),
    body,
  };
  if let Err(err) = context.response_tx.send(message) {
    panic!("Receiver dropped. {}", err);
  }
}

/// For backwards compatibility asking for the schema version.
async fn schema_establishment_phase<TRead: AsyncRead + Unpin, TWrite: AsyncWrite + Unpin>(
  stdin: &mut MessageReader<TRead>,
  stdout: &mut MessageWriter<TWrite>,
) -> Result<()> {
  // 1. An initial `0` (4 bytes) is sent asking for the schema version.
  if stdin.read_u32().await? != 0 {
    bail!("Expected a schema version request of `0`.");
  }

  // 2. The client responds with `0` (4 bytes) for success, then `4` (4 bytes) for the schema version.
  stdout.send_u32(0).await?;
  stdout.send_u32(PLUGIN_SCHEMA_VERSION).await?;
  stdout.flush().await?;

  Ok(())
}
