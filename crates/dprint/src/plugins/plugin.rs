use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::ErrBox;

pub trait Plugin : std::marker::Send + std::marker::Sync {
    /// The name of the plugin.
    fn name(&self) -> &str;
    /// The version of the plugin.
    fn version(&self) -> &str;
    /// Gets the possible keys that can be used in the configuration JSON.
    fn config_key(&self) -> &str;
    /// Gets the file extensions.
    fn file_extensions(&self) -> &Vec<String>;
    /// Gets the help url.
    fn help_url(&self) -> &str;
    /// Gets the configuration schema url.
    fn config_schema_url(&self) -> &str;
    /// Sets the configuration for the plugin.
    fn set_config(&mut self, plugin_config: HashMap<String, String>, global_config: GlobalConfiguration);
    /// Initializes the plugin.
    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox>;
}

pub trait InitializedPlugin : std::marker::Send {
    /// Gets the configuration as a collection of key value pairs.
    fn get_resolved_config(&self) -> String;
    /// Gets the configuration diagnostics.
    fn get_config_diagnostics(&self) -> Vec<ConfigurationDiagnostic>;
    /// Formats the text in memory based on the file path and file text.
    fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, String>;
}

#[cfg(test)]
pub struct TestPlugin {
    name: &'static str,
    config_key: String,
    file_extensions: Vec<String>,
    initialized_test_plugin: Option<InitializedTestPlugin>,
}

#[cfg(test)]
impl TestPlugin {
    pub fn new(name: &'static str, config_key: &'static str, file_extensions: Vec<&'static str>) -> TestPlugin {
        TestPlugin {
            name,
            config_key: String::from(config_key),
            file_extensions: file_extensions.into_iter().map(String::from).collect(),
            initialized_test_plugin: Some(InitializedTestPlugin::new()),
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
    fn set_config(&mut self, _: HashMap<String, String>, _: GlobalConfiguration) {}
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
    fn get_resolved_config(&self) -> String { String::from("{}") }
    fn get_config_diagnostics(&self) -> Vec<ConfigurationDiagnostic> { vec![] }
    fn format_text(&self, _: &PathBuf, text: &str) -> Result<String, String> {
        Ok(format!("{}_formatted", text))
    }
}
