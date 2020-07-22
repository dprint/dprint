mod types;
use types::ErrBox;
use std::io::{self, Read, Write};
use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};
use dprint_core::plugins::PluginInfo;

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

    loop {
        let (message_kind, message_data) = read_message()?;

        // todo: return an error when this fails
        // todo: return error instead of panic in some cases here (ex. unwraps)
        match message_kind {
            MessageKind::GetPluginSchemaVersion => send_int(1)?,
            MessageKind::GetPluginInfo => send_string(&serde_json::to_string(&plugin_info)?)?,
            MessageKind::GetLicenseText => send_string(license_text)?,
            MessageKind::SetGlobalConfig => {
                global_config = Some(serde_json::from_slice(&message_data)?);
                resolved_config_result.take();
                send_success()?;
            },
            MessageKind::SetPluginConfig => {
                plugin_config = Some(serde_json::from_slice(&message_data)?);
                resolved_config_result.take();

                resolved_config_result = Some(resolve_config(
                    plugin_config.as_ref().unwrap().clone(),
                    global_config.as_ref().unwrap(),
                ));
                send_success()?;
            },
            MessageKind::GetResolvedConfig => {
                let resolved_config = resolved_config_result.as_ref().unwrap();
                send_string(&serde_json::to_string(&resolved_config.config)?)?
            },
            MessageKind::GetConfigDiagnostics => {
                let resolved_config = resolved_config_result.as_ref().unwrap();
                send_string(&serde_json::to_string(&resolved_config.diagnostics)?)?
            },
            MessageKind::FormatText => {
                let message_text = String::from_utf8(message_data)?;
                let config = resolved_config_result.as_ref().unwrap();
                let separator_index = message_text.find("|").unwrap();
                let file_path = PathBuf::from(&message_text[..separator_index]);
                let file_text = &message_text[separator_index + 1..];
                match format_text(&file_path, &file_text, &config.config) {
                    Ok(formatted_text) => {
                        if formatted_text == file_text {
                            send_int(0)?; // no change
                        } else {
                            // todo: avoid copy here
                            let mut send_bytes = Vec::with_capacity(4 + formatted_text.len());
                            send_bytes.extend(&1u32.to_be_bytes());
                            send_bytes.extend(formatted_text.as_bytes());
                            send_response(&send_bytes)?;
                        }
                    }
                    Err(err) => {
                        // todo: avoid copy here
                        let error_message = err.to_string();
                        let mut send_bytes = Vec::with_capacity(4 + file_text.len());
                        send_bytes.extend(&2u32.to_be_bytes());
                        send_bytes.extend(error_message.as_bytes());
                        send_response(&send_bytes)?;
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

fn read_message() -> Result<(MessageKind, Vec<u8>), ErrBox> {
    let mut int_buf: [u8; 4] = [0; 4];
    io::stdin().read_exact(&mut int_buf)?;
    let message_kind = u32::from_be_bytes(int_buf);
    let mut int_buf: [u8; 4] = [0; 4];
    io::stdin().read_exact(&mut int_buf)?;
    let message_size = u32::from_be_bytes(int_buf);
    let message_data = if message_size > 0 {
        let mut message_data = vec![0u8; message_size as usize];
        io::stdin().read_exact(&mut message_data)?;
        message_data
    } else {
        Vec::new()
    };

    Ok((message_kind.into(), message_data))
}

fn send_success() -> Result<(), ErrBox> {
    send_response(&Vec::new())
}

fn send_string(value: &str) -> Result<(), ErrBox> {
    send_response(value.as_bytes())
}

fn send_int(value: u32) -> Result<(), ErrBox> {
    send_response(&value.to_be_bytes())
}

fn send_response(message: &[u8]) -> Result<(), ErrBox> {
    let mut int_buf: [u8; 4] = [0; 4];
    io::stdout().write_all(&mut int_buf)?; // response success

    // message length
    io::stdout().write_all(&(message.len() as u32).to_be_bytes())?;

    // message
    if !message.is_empty() {
        io::stdout().write_all(message)?;
    }

    io::stdout().flush()?;

    Ok(())
}