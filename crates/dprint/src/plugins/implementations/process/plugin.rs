use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration};
use dprint_core::plugins::{PluginInfo};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::path::PathBuf;

use crate::types::ErrBox;
use crate::plugins::{Plugin, InitializedPlugin};

pub struct ProcessPlugin {
    plugin_info: PluginInfo,
    config: Option<(HashMap<String, String>, GlobalConfiguration)>,
    executable_file_path: PathBuf,
}

impl ProcessPlugin {
    pub fn new(plugin_info: PluginInfo, executable_file_path: PathBuf) -> Self {
        ProcessPlugin {
            plugin_info,
            config: None,
            executable_file_path,
        }
    }
}

impl Plugin for ProcessPlugin {
    fn name(&self) -> &str {
        &self.plugin_info.name
    }

    fn version(&self) -> &str {
        &self.plugin_info.version
    }

    fn config_key(&self) -> &str {
        &self.plugin_info.config_key
    }

    fn file_extensions(&self) -> &Vec<String> {
        &self.plugin_info.file_extensions
    }

    fn help_url(&self) -> &str {
        &self.plugin_info.help_url
    }

    fn config_schema_url(&self) -> &str {
        &self.plugin_info.config_schema_url
    }

    fn set_config(&mut self, plugin_config: HashMap<String, String>, global_config: GlobalConfiguration) {
        self.config = Some((plugin_config, global_config));
    }

    fn get_config(&self) -> &(HashMap<String, String>, GlobalConfiguration) {
        self.config.as_ref().expect("Call set_config first.")
    }

    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let process_plugin = InitializedProcessPlugin::new(&self.executable_file_path)?;
        let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config first.");

        process_plugin.set_global_config(&global_config)?;
        process_plugin.set_plugin_config(&plugin_config)?;

        Ok(Box::new(process_plugin))
    }
}

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

enum FormatResult {
    NoChange = 0,
    Change = 1,
    Error = 2,
}

impl From<u32> for FormatResult {
    fn from(orig: u32) -> Self {
        match orig {
            0 => FormatResult::NoChange,
            1 => FormatResult::Change,
            2 => FormatResult::Error,
            _ => unreachable!(),
        }
    }
}

pub struct InitializedProcessPlugin {
    child: Arc<Mutex<Child>>,
}

impl Drop for InitializedProcessPlugin {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap();
        let _unused = child.kill();
    }
}

impl InitializedProcessPlugin {
    pub fn new(executable_file_path: &PathBuf) -> Result<Self, ErrBox> {
        let child = Command::new(executable_file_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped()) // todo: read stderr
            .stdout(Stdio::piped())
            .spawn()?;
        let initialized_plugin = InitializedProcessPlugin {
            child: Arc::new(Mutex::new(child)),
        };

        initialized_plugin.verify_plugin_schema_version()?;

        Ok(initialized_plugin)
    }

    pub fn set_global_config(&self, global_config: &GlobalConfiguration) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(global_config)?;
        self.send_data(MessageKind::SetGlobalConfig, &json)?;
        Ok(())
    }

    pub fn set_plugin_config(&self, plugin_config: &HashMap<String, String>) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(plugin_config)?;
        self.send_data(MessageKind::SetPluginConfig, &json)?;
        Ok(())
    }

    pub fn get_plugin_info(&self) -> Result<PluginInfo, ErrBox> {
        let response = self.get_bytes(MessageKind::GetPluginInfo)?;
        Ok(serde_json::from_slice(&response)?)
    }

    fn verify_plugin_schema_version(&self) -> Result<(), ErrBox> {
        let response = match self.get_bytes(MessageKind::GetPluginSchemaVersion) {
            Ok(response) => response,
            Err(err) => {
                return err!(
                    concat!(
                        "There was a problem checking the plugin schema version. ",
                        "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade. {}"
                    ),
                    err
                );
            }
        };
        let mut buf = [0u8; 4];
        buf.clone_from_slice(&response[0..4]);
        let plugin_schema_version = u32::from_be_bytes(buf);
        let expected_version = 1;
        if plugin_schema_version != expected_version {
            return err!(
                concat!(
                    "The plugin schema version was {}, but expected {}. ",
                    "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade."
                ),
                plugin_schema_version, expected_version
            );
        }

        Ok(())
    }

    fn get_string(&self, message_kind: MessageKind) -> Result<String, ErrBox> {
        let response = self.send_and_receive(message_kind, None)?;
        Ok(String::from_utf8(response)?)
    }

    fn get_bytes(&self, message_kind: MessageKind) -> Result<Vec<u8>, ErrBox> {
        self.send_and_receive(message_kind, None)
    }

    fn send_data(&self, message_kind: MessageKind, data: &[u8]) -> Result<(), ErrBox> {
        self.send_and_receive(message_kind, Some(data))?;
        Ok(())
    }

    fn send_and_receive(&self, message_kind: MessageKind, data: Option<&[u8]>) -> Result<Vec<u8>, ErrBox> {
        use std::io::{Read, Write};
        let mut child = self.child.lock().unwrap();

        // Send Message
        {
            let stdin = child.stdin.as_mut().unwrap();

            // send the kind
            stdin.write_all(&(message_kind as u32).to_be_bytes())?;

            // send the message data length or 0
            stdin.write_all(&((data.map(|data| data.len()).unwrap_or(0) as u32).to_be_bytes()))?;

            // send message data
            if let Some(data) = data {
                stdin.write_all(&data)?;
            }
        }

        // Response, read code, size, then data
        let stdout = child.stdout.as_mut().unwrap();
        let mut int_buf: [u8; 4] = [0; 4];

        // code
        stdout.read_exact(&mut int_buf)?;
        let response_kind = u32::from_be_bytes(int_buf);

        // size
        stdout.read_exact(&mut int_buf)?;
        let size = u32::from_be_bytes(int_buf);

        // message
        let mut response = vec![0u8; size as usize];
        if size > 0 {
            stdout.read_exact(&mut response)?;
        }

        // non-zero response means error
        if response_kind != 0 {
            let error_text = String::from_utf8(response)?;
            return err!("{}", error_text)
        } else {
            Ok(response)
        }
    }
}

impl InitializedPlugin for InitializedProcessPlugin {
    fn get_license_text(&self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetLicenseText)
    }

    fn get_resolved_config(&self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetResolvedConfig)
    }

    fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        let bytes = self.get_bytes(MessageKind::GetConfigDiagnostics)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, ErrBox> {
        // todo(performance): avoid copy here and drain below
        let file_path = file_path.to_string_lossy();
        let separator = "|";
        let mut send_bytes = bytes::BytesMut::with_capacity(file_path.len() + separator.len() + file_text.len());
        send_bytes.extend(file_path.as_bytes());
        send_bytes.extend(separator.as_bytes());
        send_bytes.extend(file_text.as_bytes());

        // get the response code
        let mut response_bytes = self.send_and_receive(MessageKind::FormatText, Some(&send_bytes))?;
        let mut response_code_buf = [0u8; 4];
        response_code_buf.clone_from_slice(&response_bytes[0..4]);
        let response_code = u32::from_be_bytes(response_code_buf);

        // remove response code from bytes
        response_bytes.drain(0..4);

        // handle the response
        match response_code.into() {
            FormatResult::NoChange => Ok(String::from(file_text)),
            FormatResult::Change => {
                Ok(String::from_utf8(response_bytes)?)
            }
            FormatResult::Error => {
                err!("{}", String::from_utf8(response_bytes)?)
            }
        }
    }
}
