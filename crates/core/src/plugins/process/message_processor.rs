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
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::communication::MessageReader;
use super::communication::MessageWriter;
use super::context::ProcessContext;
use super::context::StoredConfig;
use super::messages::HostFormatResponseMessageBody;
use super::messages::Message;
use super::messages::MessageBody;
use super::messages::Response;
use super::messages::ResponseBody;
use super::messages::ResponseBodyHostFormat;
use super::messages::ResponseSuccessBody;
use super::utils::setup_exit_process_panic_hook;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::plugins::AsyncPluginHandler;
use crate::plugins::CriticalFormatError;
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
  let context: ProcessContext<THandler::Configuration> = ProcessContext::new();
  let (response_tx, mut response_rx) = mpsc::unbounded_channel::<Response>();
  let host = Arc::new(ProcessHost {
    context: context.clone(),
    sender: response_tx.clone(),
  });

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
    let message = match Message::read(&mut stdin_reader).await {
      Ok(message) => message,
      Err(err) => {
        panic!(
          "YUP {}: {}",
          std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
          err
        );
      }
    };
    match message.body {
      MessageBody::Close => {
        return Ok(());
      }
      MessageBody::IsAlive => handle_message(&response_tx, message.id, || Ok(ResponseSuccessBody::Acknowledge)),
      MessageBody::GetPluginInfo => {
        handle_message(&response_tx, message.id, || {
          let plugin_info = handler.plugin_info();
          let data = serde_json::to_vec(&plugin_info)?;
          Ok(ResponseSuccessBody::Data(data))
        });
      }
      MessageBody::GetLicenseText => {
        handle_message(&response_tx, message.id, || Ok(ResponseSuccessBody::Data(handler.license_text().into_bytes())));
      }
      MessageBody::RegisterConfig(body) => {
        handle_message(&response_tx, message.id, || {
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
          Ok(ResponseSuccessBody::Acknowledge)
        });
      }
      MessageBody::ReleaseConfig(config_id) => {
        handle_message(&response_tx, message.id, || {
          context.release_config_result(config_id);
          Ok(ResponseSuccessBody::Acknowledge)
        });
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        handle_message(&response_tx, message.id, || {
          let diagnostics = context.get_config(config_id).map(|c| c.diagnostics.clone()).unwrap_or_default();
          let result = serde_json::to_vec(&*diagnostics)?;
          Ok(ResponseSuccessBody::Data(result))
        });
      }
      MessageBody::GetResolvedConfig(config_id) => {
        handle_message(&response_tx, message.id, || {
          let result = match context.get_config(config_id) {
            Some(config) => serde_json::to_vec(&*config.config)?,
            None => bail!("Did not find configuration for id: {}", config_id),
          };
          Ok(ResponseSuccessBody::Data(result))
        });
      }
      MessageBody::FormatText(body) => {
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
              send_response(
                &response_tx,
                Response {
                  id: message.id,
                  body: ResponseBody::Error(format!("Did not find configuration for id: {}", body.config_id)),
                },
              );
              continue;
            }
          },
          file_text: match String::from_utf8(body.file_text) {
            Ok(text) => text,
            Err(err) => {
              send_response(
                &response_tx,
                Response {
                  id: message.id,
                  body: ResponseBody::Error(format!("Error decoding text: {}", err)),
                },
              );
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
        let response_tx = response_tx.clone();
        tokio::task::spawn(async move {
          let result = handler.format(request, host).await;
          context.release_cancellation_token(message.id);
          if !token.is_cancelled() {
            let body = match result {
              Ok(Some(text)) => ResponseBody::Success(ResponseSuccessBody::FormatText(Some(text.into_bytes()))),
              Ok(None) => ResponseBody::Success(ResponseSuccessBody::FormatText(None)),
              Err(err) => ResponseBody::Error(err.to_string()),
            };
            send_response(&response_tx, Response { id: message.id, body });
          }
        });
      }
      MessageBody::CancelFormat(message_id) => {
        context.cancel_format(message_id);
      }
      MessageBody::HostFormatResponse(body) => {
        let data = match body {
          HostFormatResponseMessageBody::NoChange => Ok(None),
          HostFormatResponseMessageBody::Change(data) => match String::from_utf8(data) {
            Ok(data) => Ok(Some(data)),
            Err(err) => Err(anyhow!("Error deserializing success: {}", err)),
          },
          HostFormatResponseMessageBody::Error(data) => match String::from_utf8(data) {
            Ok(message) => Err(anyhow!("{}", message)),
            Err(err) => Err(anyhow!("Error deserializing error message: {}", err)),
          },
        };
        if let Some(sender) = context.take_format_host_sender(message.id) {
          sender.send(data).unwrap();
        }
      }
    }
  }
}

struct ProcessHost<TConfiguration: Serialize + Clone + Send + Sync> {
  context: ProcessContext<TConfiguration>,
  sender: UnboundedSender<Response>,
}

impl<TConfiguration: Serialize + Clone + Send + Sync> Host for ProcessHost<TConfiguration> {
  fn format(&self, request: HostFormatRequest) -> Pin<Box<dyn Future<Output = FormatResult> + Send>> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Option<String>>>();
    let id = self.context.store_format_host_sender(tx);

    // todo: start a task that listens for cancellation

    self
      .sender
      .send(Response {
        id,
        body: ResponseBody::HostFormat(ResponseBodyHostFormat {
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
        Err(err) => Err(CriticalFormatError(err.into()))?,
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
