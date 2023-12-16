use serde::Deserialize;
use serde::Serialize;

/// Information about a plugin.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
  /// The name of the plugin.
  pub name: String,
  /// The version of the plugin.
  pub version: String,
  /// Gets the key that can be used in the configuration JSON.
  pub config_key: String,
  /// A url the user can go to in order to get help information about the plugin.
  pub help_url: String,
  /// Schema url for the plugin configuration.
  ///
  /// Generally in the format: https://plugins.dprint.dev/<org-or-user>/<repo>/<tag-name>/schema.json
  /// For example: https://plugins.dprint.dev/dprint/dprint-plugin-typescript/0.60.0/schema.json
  pub config_schema_url: String,
  /// Plugin update url.
  ///
  /// Generally in the format: https://plugins.dprint.dev/<org-or-user>/<repo>/latest.json
  /// For example: https://plugins.dprint.dev/dprint/dprint-plugin-typescript/latest.json
  pub update_url: Option<String>,
}

/// The plugin file matching information based on the configuration.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileMatchingInfo {
  /// The file extensions this plugin should format.
  #[serde(default = "Vec::new")]
  pub file_extensions: Vec<String>,
  /// The file names this plugin should format.
  #[serde(default = "Vec::new")]
  pub file_names: Vec<String>,
}
