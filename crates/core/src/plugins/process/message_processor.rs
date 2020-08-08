use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::borrow::Cow;
use serde::{Serialize};

use crate::configuration::{GlobalConfiguration, ResolveConfigurationResult, ConfigKeyMap};
use crate::types::ErrBox;
use crate::plugins::PluginInfo;
use super::{MessageKind, ResponseKind, FormatResult, HostFormatResult, StdInOutReaderWriter, PLUGIN_SCHEMA_VERSION, MessagePart};

pub trait ProcessPluginHandler<TConfiguration: Clone + Serialize> {
    fn get_plugin_info(&self) -> PluginInfo;
    fn get_license_text(&self) -> &str;
    fn resolve_config(&self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
    fn format_text<'a>(
        &self,
        file_path: &PathBuf,
        file_text: &str,
        config: &TConfiguration,
        format_with_host: Box<dyn FnMut(&PathBuf, String, &ConfigKeyMap) -> Result<String, ErrBox> + 'a>
    ) -> Result<String, ErrBox>;
}

struct MessageProcessorState<TConfiguration: Clone + Serialize> {
    global_config: Option<GlobalConfiguration>,
    config: Option<ConfigKeyMap>,
    resolved_config_result: Option<ResolveConfigurationResult<TConfiguration>>,
}

/// Handles the process' messages based on the provided handler.
pub fn handle_process_stdin_stdout_messages<THandler: ProcessPluginHandler<TConfiguration>, TConfiguration: Clone + Serialize>(
    handler: THandler
) -> Result<(), ErrBox> {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader_writer = StdInOutReaderWriter::new(&mut stdin, &mut stdout);
    let mut state = MessageProcessorState {
        global_config: None,
        config: None,
        resolved_config_result: None,
    };

    loop {
        let message_kind = reader_writer.read_u32()?.into();

        match handle_message_kind(message_kind, &mut reader_writer, &handler, &mut state) {
            Err(err) => send_error_response(
                &mut reader_writer,
                &err.to_string()
            )?,
            Ok(true) => {},
            Ok(false) => return Ok(()),
        }
    }
}


fn handle_message_kind<'a, TRead: Read, TWrite: Write, TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>(
    message_kind: MessageKind,
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    handler: &THandler,
    state: &mut MessageProcessorState<TConfiguration>,
) -> Result<bool, ErrBox> {
    match message_kind {
        MessageKind::Close => return Ok(false),
        MessageKind::GetPluginSchemaVersion => send_int(reader_writer, PLUGIN_SCHEMA_VERSION)?,
        MessageKind::GetPluginInfo => send_string(reader_writer, &serde_json::to_string(&handler.get_plugin_info())?)?,
        MessageKind::GetLicenseText => send_string(reader_writer, handler.get_license_text())?,
        MessageKind::SetGlobalConfig => {
            let message_data = reader_writer.read_variable_data()?;
            state.global_config = Some(serde_json::from_slice(&message_data)?);
            state.resolved_config_result.take();
            send_success(reader_writer)?;
        },
        MessageKind::SetPluginConfig => {
            let message_data = reader_writer.read_variable_data()?;
            let plugin_config = serde_json::from_slice(&message_data)?;
            state.resolved_config_result.take();
            state.config = Some(plugin_config);
            send_success(reader_writer)?;
        },
        MessageKind::GetResolvedConfig => {
            ensure_resolved_config(handler, state)?;
            let resolved_config = get_resolved_config_result(state)?;
            send_string(reader_writer, &serde_json::to_string(&resolved_config.config)?)?
        },
        MessageKind::GetConfigDiagnostics => {
            ensure_resolved_config(handler, state)?;
            let resolved_config = get_resolved_config_result(state)?;
            send_string(reader_writer, &serde_json::to_string(&resolved_config.diagnostics)?)?
        },
        MessageKind::FormatText => {
            ensure_resolved_config(handler, state)?;
            let file_path = reader_writer.read_path_buf()?;
            let file_text = reader_writer.read_string()?;
            let override_config: ConfigKeyMap = serde_json::from_slice(&reader_writer.read_variable_data()?)?;
            let config = if !override_config.is_empty() {
                Cow::Owned(create_resolved_config_result(handler, state, override_config)?.config)
            } else {
                Cow::Borrowed(&get_resolved_config_result(state)?.config)
            };

            let mut reader_writer = reader_writer;
            let formatted_text = handler.format_text(
                &file_path,
                &file_text,
                &config,
                Box::new(|file_path, file_text, override_config| {
                    format_with_host(&mut reader_writer, file_path, file_text, override_config)
                })
            )?;

            if formatted_text == file_text {
                send_int(&mut reader_writer, FormatResult::NoChange as u32)?;
            } else {
                send_response(
                    &mut reader_writer,
                    vec![
                        MessagePart::Number(FormatResult::Change as u32),
                        MessagePart::VariableData(formatted_text.as_bytes()),
                    ]
                )?;
            }
        },
    }

    Ok(true)
}

fn ensure_resolved_config<TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>(
    handler: &THandler,
    state: &mut MessageProcessorState<TConfiguration>,
) -> Result<(), ErrBox> {
    if state.resolved_config_result.is_none() {
        state.resolved_config_result = Some(create_resolved_config_result(handler, state, HashMap::new())?);
    }

    Ok(())
}

fn create_resolved_config_result<TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>(
    handler: &THandler,
    state: &MessageProcessorState<TConfiguration>,
    override_config: ConfigKeyMap,
) -> Result<ResolveConfigurationResult<TConfiguration>, ErrBox> {
    let mut plugin_config = state.config.as_ref().ok_or("Expected plugin config to be set at this point")?.clone();
    for (key, value) in override_config {
        plugin_config.insert(key, value);
    }
    Ok(handler.resolve_config(
        plugin_config,
        state.global_config.as_ref().ok_or("Expected global config to be set at this point.")?,
    ))
}

fn get_resolved_config_result<'a, TConfiguration: Clone + Serialize>(
    state: &'a MessageProcessorState<TConfiguration>,
) -> Result<&'a ResolveConfigurationResult<TConfiguration>, ErrBox> {
    Ok(state.resolved_config_result.as_ref().ok_or("Expected the config to be resolved at this point.")?)
}

fn format_with_host<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    file_path: &PathBuf,
    file_text: String,
    override_config: &ConfigKeyMap,
) -> Result<String, ErrBox> {
    send_response(
        reader_writer,
        vec![
            MessagePart::Number(FormatResult::RequestTextFormat as u32),
            MessagePart::VariableData(file_path.to_string_lossy().as_bytes()),
            MessagePart::VariableData(file_text.as_bytes()),
            MessagePart::VariableData(&serde_json::to_vec(&override_config)?),
        ]
    )?;

    let format_result = reader_writer.read_u32()?.into();
    match format_result {
        HostFormatResult::Change => Ok(reader_writer.read_string()?),
        HostFormatResult::NoChange => Ok(file_text),
        HostFormatResult::Error => err!("{}", reader_writer.read_string()?),
    }
}

fn send_success<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
) -> Result<(), ErrBox> {
    send_response(reader_writer, Vec::new())
}

fn send_string<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    value: &str,
) -> Result<(), ErrBox> {
    send_response(reader_writer, vec![MessagePart::VariableData(value.as_bytes())])
}

fn send_int<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    value: u32,
) -> Result<(), ErrBox> {
    send_response(reader_writer, vec![MessagePart::Number(value)])
}

fn send_response<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    message_parts: Vec<MessagePart>,
) -> Result<(), ErrBox> {
    reader_writer.send_u32(ResponseKind::Success as u32)?;
    for message_part in message_parts {
        match message_part {
            MessagePart::Number(value) => reader_writer.send_u32(value)?,
            MessagePart::VariableData(value) => reader_writer.send_variable_data(value)?,
        }
    }

    Ok(())
}

fn send_error_response<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    error_message: &str,
) -> Result<(), ErrBox> {
    reader_writer.send_u32(ResponseKind::Error as u32)?;
    reader_writer.send_string(error_message)?;

    Ok(())
}
