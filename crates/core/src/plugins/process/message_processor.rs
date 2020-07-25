use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use serde::{Serialize};

use crate::configuration::{GlobalConfiguration, ResolveConfigurationResult};
use crate::types::ErrBox;
use crate::plugins::PluginInfo;
use super::{MessageKind, ResponseKind, FormatResult, HostFormatResult, StdInOutReaderWriter, PLUGIN_SCHEMA_VERSION};

pub trait ProcessPluginHandler<TConfiguration: Clone + Serialize> {
    fn get_plugin_info(&self) -> PluginInfo;
    fn get_license_text(&self) -> &str;
    fn resolve_config(&self, config: HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<TConfiguration>;
    fn format_text<'a>(
        &self,
        file_path: &PathBuf,
        file_text: &str,
        config: &TConfiguration,
        format_with_host: Box<dyn FnMut(&PathBuf, String) -> Result<String, ErrBox> + 'a>
    ) -> Result<String, ErrBox>;
}

/// Handles the process's message based on the provided handler.
pub fn handle_process_stdin_stdout_messages<THandler: ProcessPluginHandler<TConfiguration>, TConfiguration: Clone + Serialize>(
    handler: THandler
) -> Result<(), ErrBox> {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader_writer = StdInOutReaderWriter::new(&mut stdin, &mut stdout);
    let message_processor = ProcessPluginMessageProcessor::new(reader_writer, handler);

    message_processor.handle_messages()
}

struct MessageProcessorState<TConfiguration: Clone + Serialize> {
    global_config: Option<GlobalConfiguration>,
    config: Option<HashMap<String, String>>,
    resolved_config_result: Option<ResolveConfigurationResult<TConfiguration>>,
}

pub struct ProcessPluginMessageProcessor<'a, TRead: Read, TWrite: Write, TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>> {
    reader_writer: StdInOutReaderWriter<'a, TRead, TWrite>,
    handler: THandler,
    state: MessageProcessorState<TConfiguration>,
}

impl<'a, TRead: Read, TWrite: Write, TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>
    ProcessPluginMessageProcessor<'a, TRead, TWrite, TConfiguration, THandler>
{
    pub fn new(reader_writer: StdInOutReaderWriter<'a, TRead, TWrite>, handler: THandler) -> Self {
        ProcessPluginMessageProcessor {
            reader_writer,
            handler,
            state: MessageProcessorState {
                global_config: None,
                config: None,
                resolved_config_result: None,
            }
        }
    }

    pub fn handle_messages(self) -> Result<(), ErrBox> {
        let mut reader_writer = self.reader_writer;
        let handler = self.handler;
        let mut state = self.state;
        loop {
            let message_kind = reader_writer.read_message_kind()?.into();
            match handle_message_kind(message_kind, &mut reader_writer, &handler, &mut state) {
                Err(err) => send_error_response(
                    &mut reader_writer,
                    &err.to_string()
                )?,
                _ => {},
            }
        }
    }
}

fn handle_message_kind<'a, TRead: Read, TWrite: Write, TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>(
    message_kind: MessageKind,
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    handler: &THandler,
    state: &mut MessageProcessorState<TConfiguration>,
) -> Result<(), ErrBox> {
    match message_kind {
        MessageKind::GetPluginSchemaVersion => send_int(reader_writer, PLUGIN_SCHEMA_VERSION)?,
        MessageKind::GetPluginInfo => send_string(reader_writer, &serde_json::to_string(&handler.get_plugin_info())?)?,
        MessageKind::GetLicenseText => send_string(reader_writer, handler.get_license_text())?,
        MessageKind::SetGlobalConfig => {
            let message_data = reader_writer.read_message_part()?;
            state.global_config = Some(serde_json::from_slice(&message_data)?);
            state.resolved_config_result.take();
            send_success(reader_writer)?;
        },
        MessageKind::SetPluginConfig => {
            let message_data = reader_writer.read_message_part()?;
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
            let config = get_resolved_config_result(state)?;
            let file_path = reader_writer.read_message_part_as_path_buf()?;
            let file_text = reader_writer.read_message_part_as_string()?;

            let mut reader_writer = reader_writer;
            let formatted_text = handler.format_text(
                &file_path,
                &file_text,
                &config.config,
                Box::new(|file_path, file_text| {
                    format_with_host(&mut reader_writer, file_path, file_text)
                })
            )?;

            if formatted_text == file_text {
                send_int(&mut reader_writer, FormatResult::NoChange as u32)?;
            } else {
                send_response(
                    &mut reader_writer,
                    vec![
                        &(FormatResult::Change as u32).to_be_bytes(),
                        formatted_text.as_bytes()
                    ]
                )?;
            }
        }
    }

    Ok(())
}

fn ensure_resolved_config<TConfiguration: Clone + Serialize, THandler: ProcessPluginHandler<TConfiguration>>(
    handler: &THandler,
    state: &mut MessageProcessorState<TConfiguration>,
) -> Result<(), ErrBox> {
    if state.resolved_config_result.is_none() {
        state.resolved_config_result = Some(handler.resolve_config(
            state.config.as_ref().ok_or("Expected plugin config to be set at this point")?.clone(),
            state.global_config.as_ref().ok_or("Expected global config to be set at this point.")?,
        ));
    }

    Ok(())
}

fn get_resolved_config_result<'a, TConfiguration: Clone + Serialize>(
    state: &'a MessageProcessorState<TConfiguration>,
) -> Result<&'a ResolveConfigurationResult<TConfiguration>, ErrBox> {
    Ok(state.resolved_config_result.as_ref().ok_or("Expected the config to be resolved at this point.")?)
}

fn format_with_host<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    file_path: &PathBuf,
    file_text: String
) -> Result<String, ErrBox> {
    send_response(
        reader_writer,
        vec![
            &(FormatResult::RequestTextFormat as u32).to_be_bytes(),
            file_path.to_string_lossy().as_bytes(),
            file_text.as_bytes()
        ]
    )?;

    let format_result = reader_writer.read_message_part_as_u32()?.into();
    match format_result {
        HostFormatResult::Change => Ok(reader_writer.read_message_part_as_string()?),
        HostFormatResult::NoChange => Ok(file_text),
        HostFormatResult::Error => err!("{}", reader_writer.read_message_part_as_string()?),
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
    send_response(reader_writer, vec![value.as_bytes()])
}

fn send_int<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    value: u32,
) -> Result<(), ErrBox> {
    send_response(reader_writer, vec![&value.to_be_bytes()])
}

fn send_response<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    message_parts: Vec<&[u8]>
) -> Result<(), ErrBox> {
    reader_writer.send_message_kind(ResponseKind::Success as u32)?;
    for message_part in message_parts {
        reader_writer.send_message_part(message_part)?;
    }

    Ok(())
}

fn send_error_response<'a, TRead: Read, TWrite: Write>(
    reader_writer: &mut StdInOutReaderWriter<'a, TRead, TWrite>,
    error_message: &str,
) -> Result<(), ErrBox> {
    reader_writer.send_message_kind(ResponseKind::Error as u32)?;
    reader_writer.send_message_part_as_string(error_message)?;

    Ok(())
}
