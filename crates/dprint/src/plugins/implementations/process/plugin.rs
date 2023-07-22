use anyhow::Result;
use async_trait::async_trait;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;

use super::InitializedProcessPluginCommunicator;

static PLUGIN_FILE_INITIALIZE: std::sync::Once = std::sync::Once::new();

/// Use this to get an executable file name that also works in the tests.
pub fn get_test_safe_executable_path(executable_file_path: PathBuf, environment: &impl Environment) -> PathBuf {
  if environment.is_real() {
    executable_file_path
  } else {
    // do this so that we can launch the process in the tests
    let tmp_dir = PathBuf::from("temp");
    let temp_process_plugin_file = tmp_dir.join(if cfg!(target_os = "windows") { "temp-plugin.exe" } else { "temp-plugin" });
    PLUGIN_FILE_INITIALIZE.call_once(|| {
      #[allow(clippy::disallowed_methods)]
      let _ = std::fs::create_dir(&tmp_dir);
      #[allow(clippy::disallowed_methods)]
      let _ = std::fs::write(&temp_process_plugin_file, environment.read_file_bytes(&executable_file_path).unwrap());
      if cfg!(unix) {
        std::process::Command::new("sh")
          .arg("-c")
          .arg(format!("chmod +x {}", temp_process_plugin_file.to_string_lossy()))
          .status()
          .unwrap();
      }
    });
    // ignore errors if path already exists
    temp_process_plugin_file
  }
}

pub struct ProcessPlugin<TEnvironment: Environment> {
  environment: TEnvironment,
  executable_file_path: PathBuf,
  plugin_info: PluginInfo,
}

impl<TEnvironment: Environment> ProcessPlugin<TEnvironment> {
  pub fn new(environment: TEnvironment, executable_file_path: PathBuf, plugin_info: PluginInfo) -> Self {
    ProcessPlugin {
      environment,
      executable_file_path,
      plugin_info,
    }
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> Plugin for ProcessPlugin<TEnvironment> {
  fn info(&self) -> &PluginInfo {
    &self.plugin_info
  }

  fn is_process_plugin(&self) -> bool {
    true
  }

  async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>> {
    let start_instant = Instant::now();
    let plugin_name = &self.info().name;
    log_verbose!(self.environment, "Creating instance of {}", plugin_name);
    let communicator = InitializedProcessPluginCommunicator::new(plugin_name.to_string(), self.executable_file_path.clone(), self.environment.clone()).await?;
    let process_plugin = InitializedProcessPlugin::new(communicator)?;

    let result: Rc<dyn InitializedPlugin> = Rc::new(process_plugin);
    log_verbose!(
      self.environment,
      "Created instance of {} in {}ms",
      plugin_name,
      start_instant.elapsed().as_millis() as u64
    );
    Ok(result)
  }
}

pub struct InitializedProcessPlugin<TEnvironment: Environment> {
  communicator: Rc<InitializedProcessPluginCommunicator<TEnvironment>>,
}

impl<TEnvironment: Environment> InitializedProcessPlugin<TEnvironment> {
  pub fn new(communicator: InitializedProcessPluginCommunicator<TEnvironment>) -> Result<Self> {
    Ok(Self {
      communicator: Rc::new(communicator),
    })
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> InitializedPlugin for InitializedProcessPlugin<TEnvironment> {
  async fn license_text(&self) -> Result<String> {
    self.communicator.get_license_text().await
  }

  async fn resolved_config(&self, config: Arc<FormatConfig>) -> Result<String> {
    self.communicator.get_resolved_config(&config).await
  }

  async fn config_diagnostics(&self, config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>> {
    self.communicator.get_config_diagnostics(&config).await
  }

  async fn format_text(&self, request: InitializedPluginFormatRequest) -> FormatResult {
    self.communicator.format_text(request).await
  }

  async fn shutdown(&self) -> () {
    self.communicator.shutdown().await
  }
}
