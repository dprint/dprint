use crate::environment::Environment;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

// todo: remove this file? I think it's better to shutdown everything
// when a process plugin stops functioning. Previously this was used
// to restart process plugins, but that doesn't exactly work now
// that it's async. Maybe the restarting functionality should be pushed
// down and formats can be restarted on failure? Yeah. I think so.

// We only need to support having one configuration set at a time
// so hardcode this.
const CONFIG_ID: u32 = 1;

pub struct InitializedProcessPluginCommunicator<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_name: String,
  executable_file_path: PathBuf,
  config: (GlobalConfiguration, ConfigKeyMap),
  communicator: ProcessPluginCommunicator,
}

impl<TEnvironment: Environment> InitializedProcessPluginCommunicator<TEnvironment> {
  pub async fn new(environment: TEnvironment, plugin_name: String, executable_file_path: PathBuf, config: (GlobalConfiguration, ConfigKeyMap)) -> Result<Self> {
    let communicator = create_new_communicator(environment.clone(), plugin_name.clone(), &executable_file_path, &config).await?;
    let initialized_communicator = InitializedProcessPluginCommunicator {
      environment,
      plugin_name,
      executable_file_path,
      config,
      communicator,
    };

    Ok(initialized_communicator)
  }

  pub async fn get_license_text(&self) -> Result<String> {
    self.communicator.license_text().await
  }

  pub async fn get_resolved_config(&self) -> Result<String> {
    self.communicator.resolved_config(CONFIG_ID).await
  }

  pub async fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>> {
    self.communicator.config_diagnostics(CONFIG_ID).await
  }

  pub async fn format_text(
    &self,
    file_path: PathBuf,
    file_text: String,
    override_config: Option<&ConfigKeyMap>,
    range: Option<Range<usize>>,
  ) -> Result<Option<String>> {
    self.communicator.format_text(file_path, file_text, CONFIG_ID, override_config, range).await
  }
}

async fn create_new_communicator<TEnvironment: Environment>(
  environment: TEnvironment,
  plugin_name: String,
  executable_file_path: &Path,
  config: &(GlobalConfiguration, ConfigKeyMap),
) -> Result<ProcessPluginCommunicator> {
  // ensure it's initialized each time
  let communicator = ProcessPluginCommunicator::new(executable_file_path, move |error_message| {
    environment.log_stderr_with_context(&error_message, &plugin_name);
  })?;
  communicator.register_config(CONFIG_ID, &config.0, &config.1).await?;
  Ok(communicator)
}
