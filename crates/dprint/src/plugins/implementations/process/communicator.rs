use crate::environment::Environment;
use crate::plugins::PluginsCollection;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::FormatRange;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

// todo: remove this file? I think it's better to shutdown everything
// when a process plugin stops functioning. Previously this was used
// to restart process plugins, but that doesn't exactly work now
// that it's async. Maybe the restarting functionality should be pushed
// down and formats can be restarted on failure? Yeah. I think so.

// We only need to support having one configuration set at a time
// so hardcode this.
const CONFIG_ID: u32 = 1;

#[derive(Clone)]
pub struct InitializedProcessPluginCommunicator<TEnvironment: Environment> {
  // todo: investigate removing this after resolving below
  environment: TEnvironment,
  // todo: these were previously here for restarts, but I think we can remove them?
  // plugin_name: String,
  // executable_file_path: PathBuf,
  // config: (...etc...)
  communicator: ProcessPluginCommunicator,
}

impl<TEnvironment: Environment> InitializedProcessPluginCommunicator<TEnvironment> {
  pub async fn new(
    plugin_name: String,
    executable_file_path: PathBuf,
    config: (GlobalConfiguration, ConfigKeyMap),
    environment: TEnvironment,
    plugin_collection: Arc<PluginsCollection<TEnvironment>>,
  ) -> Result<Self> {
    let communicator = create_new_communicator(plugin_name.clone(), &executable_file_path, &config, environment.clone(), plugin_collection).await?;
    let initialized_communicator = InitializedProcessPluginCommunicator { environment, communicator };

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

  pub async fn format_text(&self, file_path: PathBuf, file_text: String, range: FormatRange, override_config: &ConfigKeyMap) -> Result<Option<String>> {
    self.communicator.format_text(file_path, file_text, range, CONFIG_ID, override_config).await
  }
}

async fn create_new_communicator<TEnvironment: Environment>(
  plugin_name: String,
  executable_file_path: &Path,
  config: &(GlobalConfiguration, ConfigKeyMap),
  environment: TEnvironment,
  plugin_collection: Arc<PluginsCollection<TEnvironment>>,
) -> Result<ProcessPluginCommunicator> {
  // ensure it's initialized each time
  let communicator = ProcessPluginCommunicator::new(
    executable_file_path,
    move |error_message| {
      environment.log_stderr_with_context(&error_message, &plugin_name);
    },
    plugin_collection,
  )?;
  communicator.register_config(CONFIG_ID, &config.0, &config.1).await?;
  Ok(communicator)
}
