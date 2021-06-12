use serde::{Serialize, Deserialize};

/// Information about a plugin.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    /// The name of the plugin.
    pub name: String,
    /// The version of the plugin.
    pub version: String,
    /// Gets the key that can be used in the configuration JSON.
    pub config_key: String,
    /// The file extensions this plugin should format.
    pub file_extensions: Vec<String>,
    /// The file names this plugin should format.
    #[serde(default = "Vec::new")]
    pub exact_file_names: Vec<String>,
    /// A url the user can go to in order to get help information about the plugin.
    pub help_url: String,
    /// Schema url for the plugin configuration.
    pub config_schema_url: String,
}
