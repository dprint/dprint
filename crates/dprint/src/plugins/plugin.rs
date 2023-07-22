use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use futures::future::BoxFuture;

pub trait Plugin: Send + Sync {
  fn info(&self) -> &PluginInfo;

  /// Initializes the plugin.
  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>>;

  /// Gets if this is a process plugin.
  fn is_process_plugin(&self) -> bool;
}

pub struct FormatConfig {
  pub id: FormatConfigId,
  pub raw: ConfigKeyMap,
  pub global: GlobalConfiguration,
}

pub struct InitializedPluginFormatRequest {
  pub file_path: PathBuf,
  pub file_text: String,
  pub range: FormatRange,
  pub config: Arc<FormatConfig>,
  pub override_config: ConfigKeyMap,
  pub on_host_format: HostFormatCallback,
  pub token: Arc<dyn CancellationToken>,
}

// todo: use async_trait
pub trait InitializedPlugin: Send + Sync {
  /// Gets the license text
  fn license_text(&self) -> BoxFuture<'static, Result<String>>;
  /// Gets the configuration as a collection of key value pairs.
  fn resolved_config(&self, config: Arc<FormatConfig>) -> BoxFuture<'static, Result<String>>;
  /// Gets the configuration diagnostics.
  fn config_diagnostics(&self, config: Arc<FormatConfig>) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>>;
  /// Formats the text in memory based on the file path and file text.
  fn format_text(&self, format_request: InitializedPluginFormatRequest) -> BoxFuture<'static, FormatResult>;
  /// Shuts down the plugin. This is used for process plugins.
  fn shutdown(&self) -> BoxFuture<'static, ()>;
}

#[cfg(test)]
pub struct TestPlugin {
  info: PluginInfo,
  initialized_test_plugin: Option<InitializedTestPlugin>,
}

#[cfg(test)]
impl TestPlugin {
  pub fn new(name: &str, config_key: &str, file_extensions: Vec<&str>, file_names: Vec<&str>) -> TestPlugin {
    TestPlugin {
      info: PluginInfo {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        config_key: config_key.to_string(),
        file_extensions: file_extensions.into_iter().map(String::from).collect(),
        file_names: file_names.into_iter().map(String::from).collect(),
        help_url: "https://dprint.dev/plugins/test".to_string(),
        config_schema_url: "https://plugins.dprint.dev/schemas/test.json".to_string(),
        update_url: None,
      },
      initialized_test_plugin: Some(InitializedTestPlugin::new()),
    }
  }
}

#[cfg(test)]
impl Plugin for TestPlugin {
  fn info(&self) -> &PluginInfo {
    &self.info
  }

  fn is_process_plugin(&self) -> bool {
    false
  }

  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>> {
    use futures::FutureExt;
    let test_plugin = Arc::new(self.initialized_test_plugin.clone().unwrap());
    async move {
      let result: Arc<dyn InitializedPlugin> = test_plugin;
      Ok(result)
    }
    .boxed()
  }
}

#[cfg(test)]
#[derive(Clone)]
pub struct InitializedTestPlugin {}

#[cfg(test)]
impl InitializedTestPlugin {
  pub fn new() -> InitializedTestPlugin {
    InitializedTestPlugin {}
  }
}

#[cfg(test)]
impl InitializedPlugin for InitializedTestPlugin {
  fn license_text(&self) -> BoxFuture<'static, Result<String>> {
    use futures::FutureExt;
    async move { Ok(String::from("License Text")) }.boxed()
  }

  fn resolved_config(&self, _config: Arc<FormatConfig>) -> BoxFuture<'static, Result<String>> {
    use futures::FutureExt;
    async move { Ok(String::from("{}")) }.boxed()
  }

  fn config_diagnostics(&self, _config: Arc<FormatConfig>) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>> {
    use futures::FutureExt;
    async move { Ok(vec![]) }.boxed()
  }

  fn format_text(&self, format_request: InitializedPluginFormatRequest) -> BoxFuture<'static, FormatResult> {
    use futures::FutureExt;
    async move { Ok(Some(format!("{}_formatted", format_request.file_text))) }.boxed()
  }

  fn shutdown(&self) -> BoxFuture<'static, ()> {
    Box::pin(futures::future::ready(()))
  }
}
