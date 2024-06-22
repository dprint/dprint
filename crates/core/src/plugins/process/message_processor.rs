use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use serde::Serialize;
use std::io::Read;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::context::ProcessContext;
use super::context::StoredConfig;
use super::messages::CheckConfigUpdatesMessageBody;
use super::messages::CheckConfigUpdatesResponseBody;
use super::messages::HostFormatMessageBody;
use super::messages::MessageBody;
use super::messages::ProcessPluginMessage;
use super::messages::ResponseBody;
use super::utils::setup_exit_process_panic_hook;
use super::PLUGIN_SCHEMA_VERSION;

use crate::async_runtime::FutureExt;
use crate::async_runtime::LocalBoxFuture;
use crate::communication::MessageReader;
use crate::communication::MessageWriter;
use crate::communication::SingleThreadMessageWriter;
use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::plugins::AsyncPluginHandler;
use crate::plugins::FormatRequest;
use crate::plugins::FormatResult;
use crate::plugins::HostFormatRequest;

/// Handles the process' messages based on the provided handler.
pub async fn handle_process_stdio_messages<THandler: AsyncPluginHandler>(handler: THandler) -> Result<()> {
  // ensure all process plugins exit on panic on any tokio task
  setup_exit_process_panic_hook();

  // estabilish the schema
  let (mut stdin_reader, stdout_writer) = crate::async_runtime::spawn_blocking(move || {
    let mut stdin_reader = MessageReader::new(std::io::stdin());
    let mut stdout_writer = MessageWriter::new(std::io::stdout());

    schema_establishment_phase(&mut stdin_reader, &mut stdout_writer).context("Failed estabilishing schema.")?;
    Ok::<_, anyhow::Error>((stdin_reader, stdout_writer))
  })
  .await??;

  // now start reading messages
  let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<std::io::Result<ProcessPluginMessage>>();
  crate::async_runtime::spawn_blocking(move || loop {
    let message_result = ProcessPluginMessage::read(&mut stdin_reader);
    let is_err = message_result.is_err();
    if tx.send(message_result).is_err() {
      return; // disconnected
    }
    if is_err {
      return; // shut down
    }
  });

  crate::async_runtime::spawn(async move {
    let handler = Rc::new(handler);
    let stdout_message_writer = SingleThreadMessageWriter::for_stdout(stdout_writer);
    let context: Rc<ProcessContext<THandler::Configuration>> = Rc::new(ProcessContext::new(stdout_message_writer));

    // read messages over stdin
    loop {
      let message = match rx.recv().await {
        Some(message_result) => message_result?,
        None => return Ok(()), // disconnected
      };

      match message.body {
        MessageBody::Close => {
          handle_message(&context, message.id, || Ok(MessageBody::Success(message.id)));
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
          handle_async_message(
            &context,
            message.id,
            async {
              let global_config: GlobalConfiguration = serde_json::from_slice(&body.global_config)?;
              let config_map: ConfigKeyMap = serde_json::from_slice(&body.plugin_config)?;
              let result = handler.resolve_config(config_map.clone(), global_config.clone()).await;
              context.configs.store(
                body.config_id.as_raw(),
                Rc::new(StoredConfig {
                  config: Arc::new(result.config),
                  file_matching: result.file_matching,
                  diagnostics: Rc::new(result.diagnostics),
                  config_map,
                  global_config,
                }),
              );
              Ok(MessageBody::Success(message.id))
            }
            .boxed_local(),
          )
          .await;
        }
        MessageBody::ReleaseConfig(config_id) => {
          handle_message(&context, message.id, || {
            context.configs.take(config_id.as_raw());
            Ok(MessageBody::Success(message.id))
          });
        }
        MessageBody::GetConfigDiagnostics(config_id) => {
          handle_message(&context, message.id, || {
            let diagnostics = context
              .configs
              .get_cloned(config_id.as_raw())
              .map(|c| c.diagnostics.clone())
              .unwrap_or_default();
            let data = serde_json::to_vec(&*diagnostics)?;
            Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
          });
        }
        MessageBody::GetFileMatchingInfo(config_id) => {
          handle_message(&context, message.id, || {
            let data = match context.configs.get_cloned(config_id.as_raw()) {
              Some(config) => serde_json::to_vec(&config.file_matching)?,
              None => bail!("Did not find configuration for id: {}", config_id),
            };
            Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
          });
        }
        MessageBody::GetResolvedConfig(config_id) => {
          handle_message(&context, message.id, || {
            let data = match context.configs.get_cloned(config_id.as_raw()) {
              Some(config) => serde_json::to_vec(&*config.config)?,
              None => bail!("Did not find configuration for id: {}", config_id),
            };
            Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
          });
        }
        MessageBody::CheckConfigUpdates(body_bytes) => {
          handle_async_message(
            &context,
            message.id,
            async {
              let message_body = serde_json::from_slice::<CheckConfigUpdatesMessageBody>(&body_bytes)
                .with_context(|| "Could not deserialize the check config updates message body.".to_string())?;
              let changes = handler.check_config_updates(message_body).await?;
              let response = CheckConfigUpdatesResponseBody { changes };
              let data = serde_json::to_vec(&response)?;
              Ok(MessageBody::DataResponse(ResponseBody { message_id: message.id, data }))
            }
            .boxed_local(),
          )
          .await;
        }
        MessageBody::Format(body) => {
          // now parse
          let token = Arc::new(CancellationToken::new());
          let request = FormatRequest {
            file_path: body.file_path,
            range: body.range,
            config_id: body.config_id,
            config: match context.configs.get_cloned(body.config_id.as_raw()) {
              Some(config) => {
                if body.override_config.is_empty() {
                  config.config.clone()
                } else {
                  let mut config_map = config.config_map.clone();
                  let override_config_map: ConfigKeyMap = serde_json::from_slice(&body.override_config)?;
                  for (key, value) in override_config_map {
                    config_map.insert(key, value);
                  }
                  let result = handler.resolve_config(config_map, config.global_config.clone()).await;
                  Arc::new(result.config)
                }
              }
              None => {
                send_error_response(&context, message.id, anyhow!("Did not find configuration for id: {}", body.config_id));
                continue;
              }
            },
            file_bytes: body.file_bytes,
            token: token.clone(),
          };

          // start the task
          let context = context.clone();
          let handler = handler.clone();
          let token_storage_guard = context.cancellation_tokens.store_with_owned_guard(message.id, token.clone());
          crate::async_runtime::spawn(async move {
            let original_message_id = message.id;
            let result = handler
              .format(request, {
                let context = context.clone();
                move |request| host_format(&context, original_message_id, request)
              })
              .await;
            drop(token_storage_guard);
            if !token.is_cancelled() {
              let body = match result {
                Ok(text) => MessageBody::FormatResponse(ResponseBody {
                  message_id: message.id,
                  data: text,
                }),
                Err(err) => MessageBody::Error(ResponseBody {
                  message_id: message.id,
                  data: format!("{:#}", err).into_bytes(),
                }),
              };
              send_response_body(&context, body)
            }
          });
        }
        MessageBody::CancelFormat(message_id) => {
          if let Some(token) = context.cancellation_tokens.take(message_id) {
            token.cancel();
          }
        }
        MessageBody::Error(body) => {
          let text = String::from_utf8_lossy(&body.data);
          if let Some(sender) = context.format_host_senders.take(body.message_id) {
            sender.send(Err(anyhow!("{}", text))).unwrap();
          } else {
            eprintln!("Received error from CLI. {}", text);
          }
        }
        MessageBody::FormatResponse(body) => {
          if let Some(sender) = context.format_host_senders.take(body.message_id) {
            sender.send(Ok(body.data)).unwrap();
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
  })
  .await
  .unwrap()
}

fn host_format<TConfiguration: Serialize + Clone + Send + Sync>(
  context: &ProcessContext<TConfiguration>,
  original_message_id: u32,
  request: HostFormatRequest,
) -> LocalBoxFuture<'static, FormatResult> {
  let (tx, rx) = tokio::sync::oneshot::channel::<FormatResult>();
  let id = context.id_generator.next();
  context.format_host_senders.store(id, tx);

  context
    .stdout_writer
    .send(ProcessPluginMessage {
      id,
      body: MessageBody::HostFormat(HostFormatMessageBody {
        original_message_id,
        file_path: request.file_path,
        file_text: request.file_bytes,
        range: request.range,
        override_config: serde_json::to_vec(&request.override_config).unwrap(),
      }),
    })
    .unwrap_or_else(|err| panic!("Error sending host format response: {:#}", err));

  let token = request.token;
  let stdout_writer = context.stdout_writer.clone();
  let id_generator = context.id_generator.clone();
  let original_message_id = id;

  async move {
    tokio::select! {
      _ = token.wait_cancellation() => {
        // send a cancellation to the host
        stdout_writer.send(ProcessPluginMessage {
          id: id_generator.next(),
          body: MessageBody::CancelFormat(original_message_id),
        }).unwrap_or_else(|err| panic!("Error sending host format cancellation: {:#}", err));

        // return no change
        Ok(None)
      }
      value = rx => {
        match value {
          Ok(Ok(Some(value))) => Ok(Some(value)),
          Ok(Ok(None)) => Ok(None),
          Ok(Err(err)) => Err(err),
          // means the rx was closed, so just ignore
          Err(err) => Err(err.into()),
        }
      }
    }
  }
  .boxed_local()
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

async fn handle_async_message<'a, TConfiguration: Serialize + Clone + Send + Sync>(
  context: &ProcessContext<TConfiguration>,
  original_message_id: u32,
  action: LocalBoxFuture<'a, Result<MessageBody>>,
) {
  match action.await {
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
    data: format!("{:#}", err).into_bytes(),
  });
  send_response_body(context, body)
}

fn send_response_body<TConfiguration: Serialize + Clone + Send + Sync>(context: &ProcessContext<TConfiguration>, body: MessageBody) {
  let message = ProcessPluginMessage {
    id: context.id_generator.next(),
    body,
  };
  if let Err(err) = context.stdout_writer.send(message) {
    panic!("Receiver dropped. {:#}", err);
  }
}

/// For backwards compatibility asking for the schema version.
fn schema_establishment_phase<TRead: Read + Unpin, TWrite: Write + Unpin>(stdin: &mut MessageReader<TRead>, stdout: &mut MessageWriter<TWrite>) -> Result<()> {
  // 1. An initial `0` (4 bytes) is sent asking for the schema version.
  if stdin.read_u32()? != 0 {
    bail!("Expected a schema version request of `0`.");
  }

  // 2. The client responds with `0` (4 bytes) for success
  stdout.send_u32(0)?;
  // 3. Then 4 bytes for the schema version
  stdout.send_u32(PLUGIN_SCHEMA_VERSION)?;
  stdout.flush()?;

  Ok(())
}
