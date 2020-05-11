use core::slice::{Iter, IterMut};
use std::path::PathBuf;
use std::collections::HashMap;
use super::configuration::{ConfigurationDiagnostic, GlobalConfiguration};

/// Plugin that can be implemented for use in the CLI.
pub trait Plugin : std::marker::Sync {
    /// The name of the plugin.
    fn name(&self) -> &'static str;
    /// The version of the plugin.
    fn version(&self) -> &'static str;
    /// Gets the possible keys that can be used in the configuration JSON.
    fn config_keys(&self) -> Vec<String>;
    /// Initializes the plugin.
    fn initialize(&mut self, plugin_config: HashMap<String, String>, global_config: &GlobalConfiguration);
    /// Gets whether the specified file should be formatted.
    fn should_format_file(&self, file_path: &PathBuf, file_text: &str) -> bool;
    /// Gets the configuration as a collection of key value pairs.
    fn get_resolved_config(&self) -> String;
    /// Gets the configuration diagnostics.
    fn get_configuration_diagnostics(&self) -> &Vec<ConfigurationDiagnostic>;
    /// Formats the text in memory based on the file path and file text.
    fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, String>;
}

/// A formatter constructed from a collection of plugins.
pub struct Formatter {
    plugins: Vec<Box<dyn Plugin>>,
}

impl Formatter {
    /// Creates a new formatter
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Formatter {
        Formatter { plugins }
    }

    /// Iterates over the plugins.
    pub fn iter_plugins(&self) -> Iter<'_, Box<dyn Plugin>> {
        self.plugins.iter()
    }

    /// Iterates over the plugins with a mutable iterator.
    pub fn iter_plugins_mut(&mut self) -> IterMut<'_, Box<dyn Plugin>> {
        self.plugins.iter_mut()
    }

    /// Formats the file text with one of the plugins.
    ///
    /// Returns the string when a plugin formatted or error. Otherwise None when no plugin was found.
    pub fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<Option<String>, String> {
        for plugin in self.plugins.iter() {
            if plugin.should_format_file(file_path, file_text) {
                return plugin.format_text(file_path, file_text).map(|x| Some(x));
            }
        }

        Ok(None)
    }
}
