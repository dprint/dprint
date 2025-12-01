use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::HostFormatCallback;

#[async_trait(?Send)]
pub trait Plugin: Send + Sync {
  fn info(&self) -> &PluginInfo;

  /// Initializes the plugin.
  async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>>;

  /// Gets if this is a process plugin.
  fn is_process_plugin(&self) -> bool;
}

pub struct FormatConfig {
  pub id: FormatConfigId,
  pub plugin: ConfigKeyMap,
  pub global: GlobalConfiguration,
}

pub struct InitializedPluginFormatRequest {
  pub file_path: PathBuf,
  pub file_text: Vec<u8>,
  pub range: FormatRange,
  pub config: Arc<FormatConfig>,
  pub override_config: ConfigKeyMap,
  pub on_host_format: HostFormatCallback,
  pub token: Arc<dyn CancellationToken>,
}

#[async_trait(?Send)]
pub trait InitializedPlugin {
  /// Gets the license text
  async fn license_text(&self) -> Result<String>;
  /// Gets the configuration as a collection of key value pairs.
  async fn resolved_config(&self, config: Arc<FormatConfig>) -> Result<String>;
  /// Gets the configuration's file matching info.
  async fn file_matching_info(&self, config: Arc<FormatConfig>) -> Result<FileMatchingInfo>;
  /// Gets the configuration diagnostics.
  async fn config_diagnostics(&self, config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>>;
  /// Checks for any configuration changes based on the provided plugin config.
  async fn check_config_updates(&self, message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>>;
  /// Formats the text in memory based on the file path and file text.
  async fn format_text(&self, format_request: InitializedPluginFormatRequest) -> FormatResult;
  /// Shuts down the plugin. This is used for process plugins.
  async fn shutdown(&self) -> ();
}

#[cfg(test)]
pub struct TestPlugin {
  info: PluginInfo,
  initialized_test_plugin: InitializedTestPlugin,
}

#[cfg(test)]
impl TestPlugin {
  pub fn new(name: &str, config_key: &str, file_extensions: Vec<&str>, file_names: Vec<&str>) -> TestPlugin {
    TestPlugin {
      info: PluginInfo {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        config_key: config_key.to_string(),
        help_url: "https://dprint.dev/plugins/test".to_string(),
        config_schema_url: "https://plugins.dprint.dev/schemas/test.json".to_string(),
        update_url: None,
      },
      initialized_test_plugin: InitializedTestPlugin(FileMatchingInfo {
        file_extensions: file_extensions.into_iter().map(String::from).collect(),
        file_names: file_names.into_iter().map(String::from).collect(),
      }),
    }
  }
}

#[cfg(test)]
#[async_trait(?Send)]
impl Plugin for TestPlugin {
  fn info(&self) -> &PluginInfo {
    &self.info
  }

  fn is_process_plugin(&self) -> bool {
    false
  }

  async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>> {
    let test_plugin: Rc<dyn InitializedPlugin> = Rc::new(self.initialized_test_plugin.clone());
    Ok(test_plugin)
  }
}

#[cfg(test)]
#[derive(Clone)]
pub struct InitializedTestPlugin(FileMatchingInfo);

#[cfg(test)]
#[async_trait(?Send)]
impl InitializedPlugin for InitializedTestPlugin {
  async fn license_text(&self) -> Result<String> {
    Ok(String::from("License Text"))
  }

  async fn resolved_config(&self, _config: Arc<FormatConfig>) -> Result<String> {
    Ok(String::from("{}"))
  }

  async fn file_matching_info(&self, _config: Arc<FormatConfig>) -> Result<FileMatchingInfo> {
    Ok(self.0.clone())
  }

  async fn config_diagnostics(&self, _config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>> {
    Ok(vec![])
  }

  async fn check_config_updates(&self, _message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    Ok(Vec::new())
  }

  async fn format_text(&self, format_request: InitializedPluginFormatRequest) -> FormatResult {
    Ok(Some(format!("{}_formatted", String::from_utf8(format_request.file_text)?).into_bytes()))
  }

  async fn shutdown(&self) -> () {
    // do nothing
  }
}
