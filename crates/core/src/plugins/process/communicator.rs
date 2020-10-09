use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::configuration::{ConfigKeyMap, GlobalConfiguration, ConfigurationDiagnostic};
use crate::types::ErrBox;
use crate::plugins::PluginInfo;
use super::{StdInOutReaderWriter, FormatResult, MessageKind, PLUGIN_SCHEMA_VERSION, HostFormatResult, ResponseKind, MessagePart};

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
    child: Child,
}

impl Drop for ProcessPluginCommunicator {
    fn drop(&mut self) {
        let _ignore = self.kill();
    }
}

impl ProcessPluginCommunicator {
    pub fn new(
        executable_file_path: &PathBuf,
        on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static,
    ) -> Result<Self, ErrBox> {
        ProcessPluginCommunicator::new_internal(executable_file_path, false, on_std_err)
    }

    /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
    pub fn new_with_init(
        executable_file_path: &PathBuf,
        on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static,
    ) -> Result<Self, ErrBox> {
        ProcessPluginCommunicator::new_internal(executable_file_path, true, on_std_err)
    }

    fn new_internal(
        executable_file_path: &PathBuf,
        is_init: bool,
        on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static,
    ) -> Result<Self, ErrBox> {
        let mut args = vec!["--parent-pid".to_string(), std::process::id().to_string()];
        if is_init { args.push("--init".to_string()); }

        let mut child = Command::new(executable_file_path)
            .args(&args)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            use std::io::{BufRead, ErrorKind};
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => on_std_err(line),
                    Err(err) => {
                        if err.kind() == ErrorKind::BrokenPipe {
                            return;
                        } else {
                            on_std_err(format!("Error reading line from process plugin stderr. {}", err.to_string()));
                        }
                    },
                }
            }
        });
        let mut communicator = ProcessPluginCommunicator {
            child,
        };

        communicator.verify_plugin_schema_version()?;

        Ok(communicator)
    }

    fn kill(&mut self) -> Result<(), ErrBox> {
        // attempt to exit nicely
        let _ignore = self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                MessageKind::Close,
                Vec::new()
            )
        });

        // now ensure kill
        self.child.kill()?;
        Ok(())
    }

    pub fn set_global_config(&mut self, global_config: &GlobalConfiguration) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(global_config)?;
        self.send_data(MessageKind::SetGlobalConfig, &json)?;
        Ok(())
    }

    pub fn set_plugin_config(&mut self, plugin_config: &ConfigKeyMap) -> Result<(), ErrBox> {
        let json = serde_json::to_vec(plugin_config)?;
        self.send_data(MessageKind::SetPluginConfig, &json)?;
        Ok(())
    }

    pub fn get_plugin_info(&mut self) -> Result<PluginInfo, ErrBox> {
        let response = self.get_bytes(MessageKind::GetPluginInfo)?;
        Ok(serde_json::from_slice(&response)?)
    }

    pub fn get_license_text(&mut self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetLicenseText)
    }

    pub fn get_resolved_config(&mut self) -> Result<String, ErrBox> {
        self.get_string(MessageKind::GetResolvedConfig)
    }

    pub fn get_config_diagnostics(&mut self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        let bytes = self.get_bytes(MessageKind::GetConfigDiagnostics)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn format_text(
        &mut self,
        file_path: &PathBuf,
        file_text: &str,
        override_config: &ConfigKeyMap,
        format_with_host: impl Fn(PathBuf, String, ConfigKeyMap) -> Result<Option<String>, ErrBox>,
    ) -> Result<String, ErrBox> {
        self.with_reader_writer(|reader_writer| {
            let override_config = serde_json::to_vec(override_config)?;
            // send message
            reader_writer.send_u32(MessageKind::FormatText as u32)?;
            reader_writer.send_path_buf(file_path)?;
            reader_writer.send_string(file_text)?;
            reader_writer.send_variable_data(&override_config)?;

            loop {
                read_response(reader_writer)?;

                let format_result = reader_writer.read_u32()?;
                match format_result.into() {
                    FormatResult::NoChange => break Ok(String::from(file_text)),
                    FormatResult::Change => break Ok(reader_writer.read_string()?),
                    FormatResult::RequestTextFormat => {
                        let file_path = reader_writer.read_path_buf()?;
                        let file_text = reader_writer.read_string()?;
                        let override_config = serde_json::from_slice(&reader_writer.read_variable_data()?)?;

                        match format_with_host(file_path, file_text, override_config) {
                            Ok(Some(formatted_text)) => {
                                reader_writer.send_u32(HostFormatResult::Change as u32)?;
                                reader_writer.send_string(&formatted_text)?;
                            },
                            Ok(None) => {
                                reader_writer.send_u32(HostFormatResult::NoChange as u32)?;
                            }
                            Err(err) => {
                                reader_writer.send_u32(HostFormatResult::Error as u32)?;
                                reader_writer.send_string(&err.to_string())?;
                            }
                        }
                    }
                }
            }
        })
    }

    /// Checks if the process is functioning.
    /// Only use this after an error has occurred to tell if the process should be recreated.
    pub fn is_process_alive(&mut self) -> bool {
        let result = self.get_plugin_schema_version();
        if let Ok(plugin_schema_version) = result {
            plugin_schema_version == PLUGIN_SCHEMA_VERSION
        } else {
            false
        }
    }

    fn get_plugin_schema_version(&mut self) -> Result<u32, ErrBox> {
        match self.get_u32(MessageKind::GetPluginSchemaVersion) {
            Ok(response) => Ok(response),
            Err(err) => {
                return err!(
                    concat!(
                        "There was a problem checking the plugin schema version. ",
                        "This may indicate you are using an old version of the dprint CLI or plugin and should upgrade. {}"
                    ),
                    err
                );
            }
        }
    }

    fn verify_plugin_schema_version(&mut self) -> Result<(), ErrBox> {
        let plugin_schema_version = self.get_plugin_schema_version()?;
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

    fn get_string(&mut self, message_kind: MessageKind) -> Result<String, ErrBox> {
        let bytes = self.get_bytes(message_kind)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn get_bytes(&mut self, message_kind: MessageKind) -> Result<Vec<u8>, ErrBox> {
        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                message_kind,
                Vec::new(),
            )?;
            reader_writer.read_variable_data()
        })
    }

    fn get_u32(&mut self, message_kind: MessageKind) -> Result<u32, ErrBox> {
        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                message_kind,
                Vec::new(),
            )?;
            reader_writer.read_u32()
        })
    }

    fn send_data(&mut self, message_kind: MessageKind, data: &[u8]) -> Result<(), ErrBox> {
        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                message_kind,
                vec![MessagePart::VariableData(data)],
            )
        })
    }

    fn with_reader_writer<F, FResult>(
        &mut self,
        with_action: F
    ) -> Result<FResult, ErrBox>
        where F: FnOnce(&mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>) -> Result<FResult, ErrBox>
    {
        // take because can't mutably borrow twice
        let mut stdin = self.child.stdin.take().unwrap();
        let mut stdout = self.child.stdout.take().unwrap();

        let result = {
            let mut reader_writer = StdInOutReaderWriter::new(&mut stdout, &mut stdin);

            with_action(&mut reader_writer)
        };

        self.child.stdin.replace(stdin);
        self.child.stdout.replace(stdout);

        Ok(result?)
    }
}

fn send_message(
    reader_writer: &mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>,
    message_kind: MessageKind,
    parts: Vec<MessagePart>,
) -> Result<(), ErrBox> {
    // send message
    reader_writer.send_u32(message_kind as u32)?;
    for part in parts {
        match part {
            MessagePart::VariableData(data) => reader_writer.send_variable_data(data)?,
            MessagePart::Number(value) => reader_writer.send_u32(value)?,
        }
    }

    // read response
    read_response(reader_writer)
}

fn read_response(
    reader_writer: &mut StdInOutReaderWriter<std::process::ChildStdout, std::process::ChildStdin>,
) -> Result<(), ErrBox> {
    let response_kind = reader_writer.read_u32()?;
    match response_kind.into() {
        ResponseKind::Success => Ok(()),
        ResponseKind::Error => err!("{}", reader_writer.read_string()?),
    }
}
