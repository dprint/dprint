use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;

use super::FormatResult;
use super::HostFormatResult;
use super::MessageKind;
use super::ResponseKind;
use super::StdIoMessenger;
use super::StdIoReaderWriter;
use super::PLUGIN_SCHEMA_VERSION;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::PluginInfo;
use crate::types::ErrBox;

/// Communicates with a process plugin.
pub struct ProcessPluginCommunicator {
  child: Child,
  messenger: StdIoMessenger<ChildStdout, ChildStdin>,
}

impl Drop for ProcessPluginCommunicator {
  fn drop(&mut self) {
    let _ignore = self.kill();
  }
}

impl ProcessPluginCommunicator {
  pub fn new(executable_file_path: &Path, on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static) -> Result<Self, ErrBox> {
    ProcessPluginCommunicator::new_internal(executable_file_path, false, on_std_err)
  }

  /// Provides the `--init` CLI flag to tell the process plugin to do any initialization necessary
  pub fn new_with_init(executable_file_path: &Path, on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static) -> Result<Self, ErrBox> {
    ProcessPluginCommunicator::new_internal(executable_file_path, true, on_std_err)
  }

  fn new_internal(
    executable_file_path: &Path,
    is_init: bool,
    on_std_err: impl Fn(String) + std::marker::Send + std::marker::Sync + 'static,
  ) -> Result<Self, ErrBox> {
    let mut args = vec!["--parent-pid".to_string(), std::process::id().to_string()];
    if is_init {
      args.push("--init".to_string());
    }

    let mut child = Command::new(executable_file_path)
      .args(&args)
      .stdin(Stdio::piped())
      .stderr(Stdio::piped())
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|err| err_obj!("Error starting {} with args [{}]. {}", executable_file_path.display(), args.join(" "), err))?;

    // read and output stderr prefixed
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
      use std::io::BufRead;
      use std::io::ErrorKind;
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
          }
        }
      }
    });

    let messenger = StdIoMessenger::new(StdIoReaderWriter::new(child.stdout.take().unwrap(), child.stdin.take().unwrap()));
    let mut communicator = ProcessPluginCommunicator { child, messenger };

    communicator.verify_plugin_schema_version()?;

    Ok(communicator)
  }

  fn kill(&mut self) -> Result<(), ErrBox> {
    // attempt to exit nicely
    let _ignore = self.messenger.send_message(MessageKind::Close as u32, Vec::new());

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
    file_path: &Path,
    file_text: &str,
    override_config: &ConfigKeyMap,
    format_with_host: impl Fn(PathBuf, String, ConfigKeyMap) -> Result<Option<String>, ErrBox>,
  ) -> Result<String, ErrBox> {
    let override_config = serde_json::to_vec(override_config)?;
    // send message
    self.messenger.send_message(
      MessageKind::FormatText as u32,
      vec![file_path.into(), file_text.into(), (&override_config).into()],
    )?;

    loop {
      self.messenger.read_response()?;
      let format_result = self.messenger.read_code()?;
      match format_result.into() {
        FormatResult::NoChange => {
          self.messenger.read_zero_part_message()?;
          break Ok(String::from(file_text));
        }
        FormatResult::Change => break Ok(self.messenger.read_single_part_string_message()?),
        FormatResult::RequestTextFormat => {
          let mut message_parts = self.messenger.read_multi_part_message(3)?;
          let file_path = message_parts.take_path_buf()?;
          let file_text = message_parts.take_string()?;
          let override_config = serde_json::from_slice(&message_parts.take_part()?)?;

          match format_with_host(file_path, file_text, override_config) {
            Ok(Some(formatted_text)) => {
              self
                .messenger
                .send_message(HostFormatResult::Change as u32, vec![formatted_text.as_str().into()])?;
            }
            Ok(None) => {
              self.messenger.send_message(HostFormatResult::NoChange as u32, vec![])?;
            }
            Err(err) => {
              self
                .messenger
                .send_message(HostFormatResult::Error as u32, vec![err.to_string().as_str().into()])?;
            }
          }
        }
      }
    }
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
        plugin_schema_version,
        PLUGIN_SCHEMA_VERSION
      );
    }

    Ok(())
  }

  fn get_string(&mut self, message_kind: MessageKind) -> Result<String, ErrBox> {
    let bytes = self.get_bytes(message_kind)?;
    Ok(String::from_utf8(bytes)?)
  }

  fn get_bytes(&mut self, message_kind: MessageKind) -> Result<Vec<u8>, ErrBox> {
    self.messenger.send_message(message_kind as u32, Vec::new())?;
    self.messenger.read_response()?;
    self.messenger.read_single_part_message()
  }

  fn get_u32(&mut self, message_kind: MessageKind) -> Result<u32, ErrBox> {
    self.messenger.send_message(message_kind as u32, Vec::new())?;
    self.messenger.read_response()?;
    self.messenger.read_single_part_u32_message()
  }

  fn send_data(&mut self, message_kind: MessageKind, data: &[u8]) -> Result<(), ErrBox> {
    self.messenger.send_message(message_kind as u32, vec![data.into()])?;
    self.messenger.read_response()?;
    self.messenger.read_zero_part_message()
  }
}

trait StdIoMessengerExtensions {
  fn read_response(&mut self) -> Result<(), ErrBox>;
}

impl StdIoMessengerExtensions for StdIoMessenger<ChildStdout, ChildStdin> {
  fn read_response(&mut self) -> Result<(), ErrBox> {
    let response_kind = self.read_code()?;
    match response_kind.into() {
      ResponseKind::Success => Ok(()),
      ResponseKind::Error => {
        err!("{}", self.read_single_part_error_message()?)
      }
    }
  }
}
