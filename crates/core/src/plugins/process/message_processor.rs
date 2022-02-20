use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use std::borrow::Cow;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use super::FormatResult;
use super::HostFormatResult;
use super::MessageKind;
use super::MessagePart;
use super::ResponseKind;
use super::StdIoMessenger;
use super::StdIoReaderWriter;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::GlobalConfiguration;
use crate::configuration::ResolveConfigurationResult;
use crate::plugins::PluginHandler;

struct MessageProcessorState<TConfiguration: Clone + Serialize> {
  global_config: Option<GlobalConfiguration>,
  config: Option<ConfigKeyMap>,
  resolved_config_result: Option<ResolveConfigurationResult<TConfiguration>>,
}

/// Handles the process' messages based on the provided handler.
pub fn handle_process_stdio_messages<THandler: PluginHandler<TConfiguration>, TConfiguration: Clone + Serialize>(mut handler: THandler) -> Result<()> {
  let stdin = std::io::stdin();
  let stdout = std::io::stdout();
  let reader_writer = StdIoReaderWriter::new(stdin, stdout);
  let mut messenger = StdIoMessenger::new(reader_writer);
  let mut state = MessageProcessorState {
    global_config: None,
    config: None,
    resolved_config_result: None,
  };

  loop {
    let message_kind = messenger.read_code()?.into();

    match handle_message_kind(message_kind, &mut messenger, &mut handler, &mut state) {
      Err(err) => messenger.send_error_response(&err.to_string())?,
      Ok(true) => {}
      Ok(false) => return Ok(()),
    }
  }
}

fn handle_message_kind<TRead: Read, TWrite: Write, TConfiguration: Clone + Serialize, THandler: PluginHandler<TConfiguration>>(
  message_kind: MessageKind,
  messenger: &mut StdIoMessenger<TRead, TWrite>,
  handler: &mut THandler,
  state: &mut MessageProcessorState<TConfiguration>,
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
      messenger.send_response(vec![serde_json::to_vec(&handler.get_plugin_info())?.into()])?
    }
    MessageKind::GetLicenseText => {
      messenger.read_zero_part_message()?;
      messenger.send_response(vec![handler.get_license_text().into()])?
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

      let formatted_text = handler.format_text(&file_path, &file_text, &config, |file_path, file_text, override_config| {
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

fn ensure_resolved_config<TConfiguration: Clone + Serialize, THandler: PluginHandler<TConfiguration>>(
  handler: &mut THandler,
  state: &mut MessageProcessorState<TConfiguration>,
) -> Result<()> {
  if state.resolved_config_result.is_none() {
    state.resolved_config_result = Some(create_resolved_config_result(handler, state, Default::default())?);
  }

  Ok(())
}

fn create_resolved_config_result<TConfiguration: Clone + Serialize, THandler: PluginHandler<TConfiguration>>(
  handler: &mut THandler,
  state: &MessageProcessorState<TConfiguration>,
  override_config: ConfigKeyMap,
) -> Result<ResolveConfigurationResult<TConfiguration>> {
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
