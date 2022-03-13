use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::FormatRange;
use futures::future::BoxFuture;

use crate::configuration::RawPluginConfig;

pub trait Plugin: std::marker::Send + std::marker::Sync {
  /// The name of the plugin.
  fn name(&self) -> &str;
  /// The version of the plugin.
  fn version(&self) -> &str;
  /// Gets the config key that can be used in the configuration JSON.
  fn config_key(&self) -> &str;
  /// Gets the file extensions.
  fn file_extensions(&self) -> &Vec<String>;
  /// Gets the exact file names.
  fn file_names(&self) -> &Vec<String>;
  /// Gets the help url.
  fn help_url(&self) -> &str;
  /// Gets the configuration schema url.
  fn config_schema_url(&self) -> &str;
  /// Gets the update url if it exists.
  fn update_url(&self) -> Option<&str>;
  /// Sets the configuration for the plugin.
  fn set_config(&mut self, plugin_config: RawPluginConfig, global_config: GlobalConfiguration);
  /// Initializes the plugin.
  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>>;
  /// Gets the configuration for the plugin.
  fn get_config(&self) -> &(RawPluginConfig, GlobalConfiguration);

  /// Gets a hash that represents the current state of the plugin.
  /// This is used for the "incremental" feature to tell if a plugin has changed state.
  fn get_hash(&self) -> u64 {
    let config = self.get_config();
    let mut hash_str = String::new();

    // list everything in here that would affect formatting
    hash_str.push_str(self.name());
    hash_str.push_str(self.version());

    // serialize the config keys in order to prevent the hash from changing
    let sorted_config: std::collections::BTreeMap<&String, &ConfigKeyValue> = config.0.properties.iter().collect();
    hash_str.push_str(&serde_json::to_string(&sorted_config).unwrap());

    hash_str.push_str(&serde_json::to_string(&config.0.associations).unwrap());
    hash_str.push_str(&serde_json::to_string(&config.1).unwrap());

    crate::utils::get_bytes_hash(hash_str.as_bytes())
  }
}

pub trait InitializedPlugin: Send + Sync {
  /// Gets the license text
  fn license_text(&self) -> BoxFuture<'static, Result<String>>;
  /// Gets the configuration as a collection of key value pairs.
  fn resolved_config(&self) -> BoxFuture<'static, Result<String>>;
  /// Gets the configuration diagnostics.
  fn config_diagnostics(&self) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>>;
  /// Formats the text in memory based on the file path and file text.
  fn format_text(&self, file_path: PathBuf, file_text: String, range: FormatRange, override_config: ConfigKeyMap)
    -> BoxFuture<'static, Result<Option<String>>>;
}

#[cfg(test)]
pub struct TestPlugin {
  name: &'static str,
  config_key: String,
  file_extensions: Vec<String>,
  file_names: Vec<String>,
  initialized_test_plugin: Option<InitializedTestPlugin>,
  config: (RawPluginConfig, GlobalConfiguration),
}

#[cfg(test)]
impl TestPlugin {
  pub fn new(name: &'static str, config_key: &'static str, file_extensions: Vec<&'static str>, file_names: Vec<&'static str>) -> TestPlugin {
    TestPlugin {
      name,
      config_key: String::from(config_key),
      file_extensions: file_extensions.into_iter().map(String::from).collect(),
      file_names: file_names.into_iter().map(String::from).collect(),
      initialized_test_plugin: Some(InitializedTestPlugin::new()),
      config: (
        Default::default(),
        GlobalConfiguration {
          line_width: None,
          use_tabs: None,
          indent_width: None,
          new_line_kind: None,
        },
      ),
    }
  }
}

#[cfg(test)]
impl Plugin for TestPlugin {
  fn name(&self) -> &str {
    &self.name
  }

  fn version(&self) -> &str {
    "1.0.0"
  }

  fn help_url(&self) -> &str {
    "https://dprint.dev/plugins/test"
  }

  fn config_schema_url(&self) -> &str {
    "https://plugins.dprint.dev/schemas/test.json"
  }

  fn update_url(&self) -> Option<&str> {
    None
  }

  fn config_key(&self) -> &str {
    &self.config_key
  }

  fn file_extensions(&self) -> &Vec<String> {
    &self.file_extensions
  }

  fn file_names(&self) -> &Vec<String> {
    &self.file_names
  }

  fn set_config(&mut self, _: RawPluginConfig, _: GlobalConfiguration) {}

  fn get_config(&self) -> &(RawPluginConfig, GlobalConfiguration) {
    &self.config
  }

  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>> {
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

  fn resolved_config(&self) -> BoxFuture<'static, Result<String>> {
    use futures::FutureExt;
    async move { Ok(String::from("{}")) }.boxed()
  }

  fn config_diagnostics(&self) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>> {
    use futures::FutureExt;
    async move { Ok(vec![]) }.boxed()
  }

  fn format_text(&self, _: PathBuf, text: String, range: FormatRange, _: ConfigKeyMap) -> BoxFuture<'static, Result<Option<String>>> {
    use futures::FutureExt;
    async move { Ok(Some(format!("{}_formatted", text))) }.boxed()
  }
}
