use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use futures::future::BoxFuture;
use futures::FutureExt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::configuration::RawPluginConfig;
use crate::environment::Environment;
use crate::plugins::InitializedPlugin;
use crate::plugins::Plugin;
use crate::plugins::PluginsCollection;

use super::InitializedProcessPluginCommunicator;

static PLUGIN_FILE_INITIALIZE: std::sync::Once = std::sync::Once::new();

/// Use this to get an executable file name that also works in the tests.
pub fn get_test_safe_executable_path(executable_file_path: PathBuf, environment: &impl Environment) -> PathBuf {
  if environment.is_real() {
    executable_file_path
  } else {
    // do this so that we can launch the process in the tests
    if cfg!(target_os = "windows") {
      let tmp_dir = PathBuf::from("temp");
      let temp_process_plugin_file = tmp_dir.join(if cfg!(target_os = "windows") { "temp-plugin.exe" } else { "temp-plugin" });
      PLUGIN_FILE_INITIALIZE.call_once(|| {
        let _ = std::fs::create_dir(&tmp_dir);
        let _ = std::fs::write(&temp_process_plugin_file, environment.read_file_bytes(&executable_file_path).unwrap());
      });
      // ignore errors if path already exists
      temp_process_plugin_file
    } else {
      // couldn't figure out how to do chmod +x on a file in rust
      PathBuf::from("../../target/release/test-process-plugin")
    }
  }
}

pub struct ProcessPlugin<TEnvironment: Environment> {
  environment: TEnvironment,
  executable_file_path: PathBuf,
  plugin_info: PluginInfo,
  config: Option<(RawPluginConfig, GlobalConfiguration)>,
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
}

impl<TEnvironment: Environment> ProcessPlugin<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    executable_file_path: PathBuf,
    plugin_info: PluginInfo,
    plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  ) -> Self {
    ProcessPlugin {
      environment,
      executable_file_path,
      plugin_info,
      config: None,
      plugins_collection,
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

  fn file_names(&self) -> &Vec<String> {
    &self.plugin_info.file_names
  }

  fn help_url(&self) -> &str {
    &self.plugin_info.help_url
  }

  fn config_schema_url(&self) -> &str {
    &self.plugin_info.config_schema_url
  }

  fn update_url(&self) -> Option<&str> {
    self.plugin_info.update_url.as_deref()
  }

  fn set_config(&mut self, plugin_config: RawPluginConfig, global_config: GlobalConfiguration) {
    self.config = Some((plugin_config, global_config));
  }

  fn get_config(&self) -> &(RawPluginConfig, GlobalConfiguration) {
    self.config.as_ref().expect("Call set_config first.")
  }

  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>> {
    let config = self.config.as_ref().expect("Call set_config first.");
    let plugin_name = self.plugin_info.name.clone();
    let executable_file_path = self.executable_file_path.clone();
    let config = (config.1.clone(), config.0.properties.clone());
    let environment = self.environment.clone();
    let plugins_collection = self.plugins_collection.clone();
    async move {
      let communicator =
        InitializedProcessPluginCommunicator::new(plugin_name, executable_file_path, config, environment.clone(), plugins_collection.clone()).await?;
      let process_plugin = InitializedProcessPlugin::new(communicator)?;

      let result: Arc<dyn InitializedPlugin> = Arc::new(process_plugin);
      Ok(result)
    }
    .boxed()
  }
}

#[derive(Clone)]
pub struct InitializedProcessPlugin<TEnvironment: Environment> {
  communicator: InitializedProcessPluginCommunicator<TEnvironment>,
}

impl<TEnvironment: Environment> InitializedProcessPlugin<TEnvironment> {
  pub fn new(communicator: InitializedProcessPluginCommunicator<TEnvironment>) -> Result<Self> {
    Ok(Self { communicator })
  }
}

impl<TEnvironment: Environment> InitializedPlugin for InitializedProcessPlugin<TEnvironment> {
  fn license_text(&self) -> BoxFuture<'static, Result<String>> {
    let communicator = self.communicator.clone();
    async move { communicator.get_license_text().await }.boxed()
  }

  fn resolved_config(&self) -> BoxFuture<'static, Result<String>> {
    let communicator = self.communicator.clone();
    async move { communicator.get_resolved_config().await }.boxed()
  }

  fn config_diagnostics(&self) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>> {
    let communicator = self.communicator.clone();
    async move { communicator.get_config_diagnostics().await }.boxed()
  }

  fn format_text(&self, file_path: PathBuf, file_text: String, range: FormatRange, override_config: ConfigKeyMap) -> BoxFuture<'static, Result<FormatResult>> {
    // todo: this used to recreate the process if dead... this needs to be redesigned
    let communicator = self.communicator.clone();
    async move { communicator.format_text(file_path, file_text, range, &override_config).await }.boxed()
  }
}
