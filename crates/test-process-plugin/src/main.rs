use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};
use dprint_core::types::ErrBox;
use dprint_core::plugins::PluginInfo;
use dprint_core::process::StdInOutReaderWriter;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

/// todo: share with CLI (maybe in dprint_core)
enum MessageKind {
    GetPluginSchemaVersion = 0,
    GetPluginInfo = 1,
    GetLicenseText = 2,
    GetResolvedConfig = 3,
    SetGlobalConfig = 4,
    SetPluginConfig = 5,
    GetConfigDiagnostics = 6,
    FormatText = 7,
}

enum ResponseKind {
    Success = 0,
    Error = 1,
}

// todo: generate with a macro
impl From<u32> for MessageKind {
    fn from(kind: u32) -> Self {
        match kind {
            0 => MessageKind::GetPluginSchemaVersion,
            1 => MessageKind::GetPluginInfo,
            2 => MessageKind::GetLicenseText,
            3 => MessageKind::GetResolvedConfig,
            4 => MessageKind::SetGlobalConfig,
            5 => MessageKind::SetPluginConfig,
            6 => MessageKind::GetConfigDiagnostics,
            7 => MessageKind::FormatText,
            _ => unreachable!(), // todo: return a result and say the provided value
        }
    }
}

enum FormatResult {
    NoChange = 0,
    Change = 1,
    Error = 2,
}

fn main() -> Result<(), ErrBox> {
    let plugin_info = PluginInfo {
        name: "test-process-plugin".to_string(),
        version: "0.2.0".to_string(),
        config_key: "testProcessPlugin".to_string(),
        file_extensions: vec!["txt_ps".to_string()],
        help_url: "test-process-plugin-help-url".to_string(),
        config_schema_url: "".to_string()
    };
    let license_text = "License text.";
    let mut global_config: Option<GlobalConfiguration> = None;
    let mut plugin_config: Option<HashMap<String, String>> = None;
    let mut resolved_config_result: Option<ResolveConfigurationResult<Configuration>> = None;

    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader_writer = StdInOutReaderWriter::new(&mut stdin, &mut stdout);

    loop {
        let message_kind = reader_writer.read_message_kind()?.into();

        // todo: return an error when this fails
        // todo: return error instead of panic in some cases here (ex. unwraps)
        match message_kind {
            MessageKind::GetPluginSchemaVersion => send_int(&mut reader_writer, 1)?,
            MessageKind::GetPluginInfo => send_string(&mut reader_writer, &serde_json::to_string(&plugin_info)?)?,
            MessageKind::GetLicenseText => send_string(&mut reader_writer, license_text)?,
            MessageKind::SetGlobalConfig => {
                let message_data = reader_writer.read_message_part()?;
                global_config = Some(serde_json::from_slice(&message_data)?);
                resolved_config_result.take();
                send_success(&mut reader_writer)?;
            },
            MessageKind::SetPluginConfig => {
                let message_data = reader_writer.read_message_part()?;
                plugin_config = Some(serde_json::from_slice(&message_data)?);
                resolved_config_result.take();

                resolved_config_result = Some(resolve_config(
                    plugin_config.as_ref().unwrap().clone(),
                    global_config.as_ref().unwrap(),
                ));
                send_success(&mut reader_writer)?;
            },
            MessageKind::GetResolvedConfig => {
                let resolved_config = resolved_config_result.as_ref().unwrap();
                send_string(&mut reader_writer, &serde_json::to_string(&resolved_config.config)?)?
            },
            MessageKind::GetConfigDiagnostics => {
                let resolved_config = resolved_config_result.as_ref().unwrap();
                send_string(&mut reader_writer, &serde_json::to_string(&resolved_config.diagnostics)?)?
            },
            MessageKind::FormatText => {
                let config = resolved_config_result.as_ref().unwrap();

                let message_data = reader_writer.read_message_part()?;
                let file_path = PathBuf::from(std::str::from_utf8(&message_data).unwrap());

                let file_text = reader_writer.read_message_part_as_string()?;

                match format_text(&file_path, &file_text, &config.config) {
                    Ok(formatted_text) => {
                        if formatted_text == file_text {
                            send_int(&mut reader_writer, 0)?; // no change
                        } else {
                            send_response(
                                &mut reader_writer,
                                vec![
                                    &(1 as u32).to_be_bytes(), // change
                                    formatted_text.as_bytes()
                                ]
                            )?;
                        }
                    }
                    Err(err) => {
                        send_response(
                            &mut reader_writer,
                            vec![
                                &(2 as u32).to_be_bytes(), // error
                                err.to_string().as_bytes()
                            ]
                        )?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_text(_: &PathBuf, file_text: &str, config: &Configuration) -> Result<String, String> {
    /*if file_text.starts_with("plugin: ") {
        format_with_host(&PathBuf::from("./test.txt"), file_text.replace("plugin: ", ""))
    } else */if file_text == "should_error" {
        Err(String::from("Did error."))
    } else if file_text.ends_with(&config.ending) {
        Ok(String::from(file_text))
    } else {
        Ok(format!("{}_{}", file_text, config.ending))
    }
}

fn resolve_config(config: HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut config = config;
    let ending = config.remove("ending").unwrap_or(String::from("formatted"));
    let line_width = config.remove("line_width").map(|x| x.parse::<u32>().unwrap()).unwrap_or(global_config.line_width.unwrap_or(120));
    let mut diagnostics = Vec::new();

    diagnostics.extend(get_unknown_property_diagnostics(config));

    ResolveConfigurationResult {
        config: Configuration { ending, line_width },
        diagnostics,
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