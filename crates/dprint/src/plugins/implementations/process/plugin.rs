use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration};
use dprint_core::plugins::PluginInfo;
use dprint_core::process::{MessageKind, FormatResult, HostFormatResult, ResponseKind, StdInOutReaderWriter};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::path::PathBuf;

use crate::environment::Environment;
use crate::plugins::{Plugin, InitializedPlugin, PluginPools};
use crate::types::ErrBox;

use super::super::format_with_plugin_pool;

pub struct ProcessPlugin<TEnvironment: Environment> {
    executable_file_path: PathBuf,
    plugin_info: PluginInfo,
    config: Option<(HashMap<String, String>, GlobalConfiguration)>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> ProcessPlugin<TEnvironment> {
    pub fn new(executable_file_path: PathBuf, plugin_info: PluginInfo, plugin_pools: Arc<PluginPools<TEnvironment>>) -> Self {
        ProcessPlugin {
            executable_file_path,
            plugin_info,
            config: None,
            plugin_pools
        }
    }
}

impl<TEnvironment: Environment> Plugin for ProcessPlugin<TEnvironment> {
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
        let process_plugin = InitializedProcessPlugin::new(
            self.name().to_string(),
            &self.executable_file_path,
            Some(self.plugin_pools.clone())
        )?;
        let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config first.");

        process_plugin.set_global_config(&global_config)?;
        process_plugin.set_plugin_config(&plugin_config)?;

        Ok(Box::new(process_plugin))
    }
}

pub struct InitializedProcessPlugin<TEnvironment: Environment> {
    name: String,
    child: Arc<Mutex<Child>>,
    plugin_pools: Option<Arc<PluginPools<TEnvironment>>>,
}

impl<TEnvironment: Environment> Drop for InitializedProcessPlugin<TEnvironment> {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap();
        let _unused = child.kill();
    }
}

impl<TEnvironment: Environment> InitializedProcessPlugin<TEnvironment> {
    pub fn new(
        name: String,
        executable_file_path: &PathBuf,
        plugin_pools: Option<Arc<PluginPools<TEnvironment>>>,
    ) -> Result<Self, ErrBox> {
        let child = Command::new(executable_file_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped()) // todo: read stderr
            .stdout(Stdio::piped())
            .spawn()?;
        let initialized_plugin = InitializedProcessPlugin {
            name,
            plugin_pools,
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

            with_action(&mut reader_writer)?
        };

        // don't bother replacing these on error above, because it will be exiting anyway
        child.stdin.replace(stdin);
        child.stdout.replace(stdout);

        Ok(result)
    }
}

impl<TEnvironment: Environment> InitializedPlugin for InitializedProcessPlugin<TEnvironment> {
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
        let file_path = file_path.to_string_lossy();

        self.with_reader_writer(|reader_writer| {
            send_message(
                reader_writer,
                MessageKind::FormatText, vec![
                    file_path.as_bytes(),
                    file_text.as_bytes()
                ]
            )?;

            loop {
                let response_code = reader_writer.read_message_part_as_u32()?;
                match response_code.into() {
                    FormatResult::NoChange => break Ok(String::from(file_text)),
                    FormatResult::Change => break Ok(reader_writer.read_message_part_as_string()?),
                    FormatResult::Error => break err!("{}", reader_writer.read_message_part_as_string()?),
                    FormatResult::RequestTextFormat => {
                        let file_path = reader_writer.read_message_part_as_path_buf()?;
                        let file_text = reader_writer.read_message_part_as_string()?;
                        let pools = self.plugin_pools.as_ref().unwrap();

                        match format_with_plugin_pool(&self.name, &file_path, &file_text, pools) {
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
    let response_kind = reader_writer.read_message_kind()?.into();
    match response_kind {
        ResponseKind::Success => Ok(()),
        ResponseKind::Error => err!("{}", reader_writer.read_message_part_as_string()?),
    }
}
