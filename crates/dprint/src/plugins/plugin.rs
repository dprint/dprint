use std::path::Path;

use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration, ConfigKeyMap, ConfigKeyValue};
use dprint_core::types::ErrBox;

pub trait Plugin : std::marker::Send + std::marker::Sync {
    /// The name of the plugin.
    fn name(&self) -> &str;
    /// The version of the plugin.
    fn version(&self) -> &str;
    /// Gets the config key that can be used in the configuration JSON.
    fn config_key(&self) -> &str;
    /// Gets the file extensions.
    fn file_extensions(&self) -> &Vec<String>;
    /// Gets the help url.
    fn help_url(&self) -> &str;
    /// Gets the configuration schema url.
    fn config_schema_url(&self) -> &str;
    /// Sets the configuration for the plugin.
    fn set_config(&mut self, plugin_config: ConfigKeyMap, global_config: GlobalConfiguration);
    /// Initializes the plugin.
    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox>;
    /// Gets the configuration for the plugin.
    fn get_config(&self) -> &(ConfigKeyMap, GlobalConfiguration);

    /// Gets a hash that represents the current state of the plugin.
    /// This is used for the "incremental" feature to tell if a plugin has changed state.
    fn get_hash(&self) -> u64 {
        let config = self.get_config();
        let mut hash_str = String::new();

        // list everything in here that would affect formatting
        hash_str.push_str(&self.name());
        hash_str.push_str(&self.version());

        // serialize the config keys in order to prevent the hash from changing
        let sorted_config: std::collections::BTreeMap::<&String, &ConfigKeyValue> = config.0.iter().collect();
        hash_str.push_str(&serde_json::to_string(&sorted_config).unwrap());

        hash_str.push_str(&serde_json::to_string(&config.1).unwrap());

        crate::utils::get_bytes_hash(hash_str.as_bytes())
    }
}

pub trait InitializedPlugin : std::marker::Send {
    /// Gets the license text
    fn get_license_text(&self) -> Result<String, ErrBox>;
    /// Gets the configuration as a collection of key value pairs.
    fn get_resolved_config(&self) -> Result<String, ErrBox>;
    /// Gets the configuration diagnostics.
    fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox>;
    /// Formats the text in memory based on the file path and file text.
    fn format_text(&mut self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> Result<String, ErrBox>;
}

#[cfg(test)]
pub struct TestPlugin {
    name: &'static str,
    config_key: String,
    file_extensions: Vec<String>,
    initialized_test_plugin: Option<InitializedTestPlugin>,
    config: (ConfigKeyMap, GlobalConfiguration),
}

#[cfg(test)]
impl TestPlugin {
    pub fn new(name: &'static str, config_key: &'static str, file_extensions: Vec<&'static str>) -> TestPlugin {
        TestPlugin {
            name,
            config_key: String::from(config_key),
            file_extensions: file_extensions.into_iter().map(String::from).collect(),
            initialized_test_plugin: Some(InitializedTestPlugin::new()),
            config: (std::collections::HashMap::new(), GlobalConfiguration {
                line_width: None,
                use_tabs: None,
                indent_width: None,
                new_line_kind: None,
            })
        }
    }
}

#[cfg(test)]
impl Plugin for TestPlugin {
    fn name(&self) -> &str { &self.name }
    fn version(&self) -> &str { "1.0.0" }
    fn help_url(&self) -> &str { "https://dprint.dev/plugins/test" }
    fn config_schema_url(&self) -> &str { "https://plugins.dprint.dev/schemas/test.json" }
    fn config_key(&self) -> &str { &self.config_key }
    fn file_extensions(&self) -> &Vec<String> { &self.file_extensions }
    fn set_config(&mut self, _: ConfigKeyMap, _: GlobalConfiguration) {}
    fn get_config(&self) -> &(ConfigKeyMap, GlobalConfiguration) {
        &self.config
    }
    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        Ok(Box::new(self.initialized_test_plugin.clone().unwrap()))
    }
}

#[cfg(test)]
#[derive(Clone)]
pub struct InitializedTestPlugin {
}

#[cfg(test)]
impl InitializedTestPlugin {
    pub fn new() -> InitializedTestPlugin {
        InitializedTestPlugin {}
    }
}

#[cfg(test)]
impl InitializedPlugin for InitializedTestPlugin {
    fn get_license_text(&self) -> Result<String, ErrBox> { Ok(String::from("License Text")) }
    fn get_resolved_config(&self) -> Result<String, ErrBox> { Ok(String::from("{}")) }
    fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> { Ok(vec![]) }
    fn format_text(&mut self, _: &Path, text: &str, _: &ConfigKeyMap) -> Result<String, ErrBox> {
        Ok(format!("{}_formatted", text))
    }
}
