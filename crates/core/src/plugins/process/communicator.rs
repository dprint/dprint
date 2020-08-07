use std::path::PathBuf;
use std::sync::Mutex;
use std::process::{Child, Command, Stdio};

use crate::configuration::{ConfigKeyMap, GlobalConfiguration, ConfigurationDiagnostic};
use crate::types::ErrBox;
use crate::plugins::PluginInfo;
use super::{StdInOutReaderWriter, FormatResult, MessageKind, PLUGIN_SCHEMA_VERSION, HostFormatResult, ResponseKind};

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
    child: Mutex<Child>,
}

impl Drop for ProcessPluginCommunicator {
    fn drop(&mut self) {
        let _ignore = self.kill();
    }
}

impl ProcessPluginCommunicator {
    pub fn new(executable_file_path: &PathBuf) -> Result<Self, ErrBox> {
        let child = Command::new(executable_file_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped())
            .spawn()?;
        let communicator = ProcessPluginCommunicator {
            child: Mutex::new(child),
        };

        communicator.verify_plugin_schema_version()?;

        Ok(communicator)
    }

    fn kill(&self) -> Result<(), ErrBox> {
        // attempt to exit nicely
        let _ignore = self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                MessageKind::Close,
                Vec::new()
            )
        });

        // now ensure kill
        let mut child = self.child.lock().unwrap();
        child.kill()?;
        Ok(())
    }

    pub fn set_global_config(&self, global_config: &GlobalConfiguration) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(global_config)?;
        self.send_data(MessageKind::SetGlobalConfig, &json)?;
        Ok(())
    }

    pub fn set_plugin_config(&self, plugin_config: &ConfigKeyMap) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(plugin_config)?;
        self.send_data(MessageKind::SetPluginConfig, &json)?;
        Ok(())
    }

    pub fn get_plugin_info(&self) -> Result<PluginInfo, ErrBox> {
        let response = self.get_bytes(MessageKind::GetPluginInfo)?;
        Ok(serde_json::from_slice(&response)?)
    }

    pub fn get_license_text(&self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetLicenseText)
    }

    pub fn get_resolved_config(&self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetResolvedConfig)
    }

    pub fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        let bytes = self.get_bytes(MessageKind::GetConfigDiagnostics)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn format_text(
        &self,
        file_path: &PathBuf,
        file_text: &str,
        override_config: &ConfigKeyMap,
        format_with_host: impl Fn(PathBuf, String, ConfigKeyMap) -> Result<Option<String>, ErrBox>,
    ) -> Result<String, ErrBox> {
        self.with_reader_writer(|reader_writer| {
            let override_config = serde_json::to_vec(override_config)?;
            // send message
            reader_writer.send_message_kind(MessageKind::FormatText as u32)?;
            reader_writer.send_message_part_as_path_buf(file_path)?;
            reader_writer.send_message_part_as_string(file_text)?;
            reader_writer.send_message_part(&override_config)?;

            loop {
                read_response(reader_writer)?;

                let format_result = reader_writer.read_message_part_as_u32()?;
                match format_result.into() {
                    FormatResult::NoChange => break Ok(String::from(file_text)),
                    FormatResult::Change => break Ok(reader_writer.read_message_part_as_string()?),
                    FormatResult::RequestTextFormat => {
                        let file_path = reader_writer.read_message_part_as_path_buf()?;
                        let file_text = reader_writer.read_message_part_as_string()?;
                        let override_config = serde_json::from_slice(&reader_writer.read_message_part()?)?;

                        match format_with_host(file_path, file_text, override_config) {
                            Ok(Some(formatted_text)) => {
                                reader_writer.send_message_part_as_u32(HostFormatResult::Change as u32)?;
                                reader_writer.send_message_part_as_string(&formatted_text)?;
                            },
                            Ok(None) => {
                                reader_writer.send_message_part_as_u32(HostFormatResult::NoChange as u32)?;
                            }
                            Err(err) => {
                                reader_writer.send_message_part_as_u32(HostFormatResult::Error as u32)?;
                                reader_writer.send_message_part_as_string(&err.to_string())?;
                            }
                        }
                    }
                }
            }
        })
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
        if plugin_schema_version != PLUGIN_SCHEMA_VERSION {
            return err!(
                concat!(
                    "The plugin schema version was {}, but expected {}. ",
                    "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade."
                ),
                plugin_schema_version, PLUGIN_SCHEMA_VERSION
            );
        }

        Ok(())
    }

    fn get_string(&self, message_kind: MessageKind) -> Result<String, ErrBox> {
        let bytes = self.get_bytes(message_kind)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn get_bytes(&self, message_kind: MessageKind) -> Result<Vec<u8>, ErrBox> {
        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                message_kind,
                Vec::new(),
            )?;
            reader_writer.read_message_part()
        })
    }

    fn send_data(&self, message_kind: MessageKind, data: &[u8]) -> Result<(), ErrBox> {
        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                message_kind,
                vec![data],
            )
        })
    }

    fn with_reader_writer<F, FResult>(
        &self,
        with_action: F
    ) -> Result<FResult, ErrBox>
        where F: FnOnce(&mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>) -> Result<FResult, ErrBox>
    {
        let mut child = self.child.lock().unwrap();
        // take because can't mutably borrow twice
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        let result = {
            let mut reader_writer = StdInOutReaderWriter::new(&mut stdout, &mut stdin);

            with_action(&mut reader_writer)
        };

        child.stdin.replace(stdin);
        child.stdout.replace(stdout);

        Ok(result?)
    }
}

fn send_message(
    reader_writer: &mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>,
    message_kind: MessageKind,
    parts: Vec<&[u8]>,
) -> Result<(), ErrBox> {
    // send message
    reader_writer.send_message_kind(message_kind as u32)?;
    for part in parts {
        reader_writer.send_message_part(part)?;
    }

    // read response
    read_response(reader_writer)
}

fn read_response(
    reader_writer: &mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>,
) -> Result<(), ErrBox> {
    let response_kind = reader_writer.read_message_kind()?;
    match response_kind.into() {
        ResponseKind::Success => Ok(()),
        ResponseKind::Error => err!("{}", reader_writer.read_message_part_as_string()?),
    }
}
