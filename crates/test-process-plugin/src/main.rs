use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};
use dprint_core::err;
use dprint_core::types::ErrBox;
use dprint_core::plugins::PluginInfo;
use dprint_core::process::{MessageKind, ResponseKind, FormatResult, HostFormatResult, StdInOutReaderWriter, PLUGIN_SCHEMA_VERSION};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

// todo: resolve todos and extract out some of this to dprint_core to make implementing this very easy in Rust

fn main() -> Result<(), ErrBox> {
    let plugin_info = PluginInfo {
        name: String::from(env!("CARGO_PKG_NAME")),
        version: String::from(env!("CARGO_PKG_VERSION")),
        config_key: "testProcessPlugin".to_string(),
        file_extensions: vec!["txt_ps".to_string()],
        help_url: "https://dprint.dev/plugins/test-process".to_string(),
        config_schema_url: "".to_string()
    };
    let license_text = "License text.";
    let mut global_config: Option<GlobalConfiguration> = None;
    let mut resolved_config_result: Option<ResolveConfigurationResult<Configuration>> = None;

    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader_writer = StdInOutReaderWriter::new(&mut stdin, &mut stdout);

    loop {
        let message_kind = reader_writer.read_message_kind()?.into();

        // todo: return an error when this fails
        // todo: return error instead of panic in some cases here (ex. unwraps)
        match message_kind {
            MessageKind::GetPluginSchemaVersion => send_int(&mut reader_writer, PLUGIN_SCHEMA_VERSION)?,
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
                let plugin_config = serde_json::from_slice(&message_data)?;
                resolved_config_result.take();

                resolved_config_result = Some(resolve_config(
                    plugin_config,
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

                let file_path = reader_writer.read_message_part_as_path_buf()?;
                let file_text = reader_writer.read_message_part_as_string()?;

                match format_text(&mut reader_writer, &file_path, &file_text, &config.config) {
                    Ok(formatted_text) => {
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
                    Err(err) => {
                        send_response(
                            &mut reader_writer,
                            vec![
                                &(FormatResult::Error as u32).to_be_bytes(), // error
                                err.to_string().as_bytes()
                            ]
                        )?;
                    }
                }
            }
        }
    }
}

fn format_text(
    reader_writer: &mut StdInOutReaderWriter<std::io::Stdin, std::io::Stdout>,
    _: &PathBuf,
    file_text: &str,
    config: &Configuration,
) -> Result<String, ErrBox> {
    if file_text.starts_with("plugin: ") {
        format_with_host(reader_writer, &PathBuf::from("./test.txt"), file_text.replace("plugin: ", ""))
    } else if file_text == "should_error" {
        err!("Did error")
    } else if file_text.ends_with(&config.ending) {
        Ok(String::from(file_text))
    } else {
        Ok(format!("{}_{}", file_text, config.ending))
    }
}

fn format_with_host(
    reader_writer: &mut StdInOutReaderWriter<std::io::Stdin, std::io::Stdout>,
    file_path: &PathBuf,
    file_text: String
) -> Result<String, ErrBox> {
    reader_writer.send_message_part_as_u32(FormatResult::RequestTextFormat as u32)?;
    reader_writer.send_message_part_as_path_buf(&file_path)?;
    reader_writer.send_message_part_as_string(&file_text)?;

    let format_result = reader_writer.read_message_part_as_u32()?.into();
    match format_result {
        HostFormatResult::Change => Ok(reader_writer.read_message_part_as_string()?),
        HostFormatResult::NoChange => Ok(file_text),
        HostFormatResult::Error => err!("{}", reader_writer.read_message_part_as_string()?),
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
