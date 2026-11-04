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
  /// Whether the plugin formats the range of a format request, which plugins
  /// should always provide.
  ///
  /// - `Some(true)` - Editors give the plugin the range as it is and use what
  ///   it formats, so it decides what to format around the range (ex. the
  ///   nodes it overlaps).
  /// - `Some(false)` - Editors don't format a range with the plugin.
  /// - `None` - Only for plugins made before this existed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub supports_range_formatting: Option<bool>,
}

/// What the dprint-core a plugin was built with supports, which is sent with
/// the plugin info. A plugin doesn't provide these itself.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilities {
  /// Whether a format request may have several ranges (see `FormatRanges`).
  /// Otherwise the host formats one range at a time.
  #[serde(default)]
  pub multi_range_format: bool,
}

impl PluginCapabilities {
  /// What this version of dprint-core supports.
  pub const CURRENT: PluginCapabilities = PluginCapabilities { multi_range_format: true };
}

/// The plugin info as a plugin sends it: the [`PluginInfo`] the plugin provides
/// with the [`PluginCapabilities`] of the dprint-core it was built with.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoMessage {
  #[serde(flatten)]
  pub info: PluginInfo,
  /// Plugins made before this existed don't send it.
  #[serde(default)]
  pub capabilities: PluginCapabilities,
}

impl PluginInfoMessage {
  /// The message for a plugin built with this version of dprint-core.
  pub fn current(info: PluginInfo) -> Self {
    PluginInfoMessage {
      info,
      capabilities: PluginCapabilities::CURRENT,
    }
  }
}

/// Message for when a plugin's [`FileMatchingInfo`] can't be deserialized. A
/// newer plugin may describe its file matching in a way this version of the CLI
/// doesn't understand, so point at upgrading rather than leaving the user with
/// only a deserialization error.
pub const FILE_MATCHING_INFO_ERROR_MESSAGE: &str =
  "Failed getting the plugin's file matching info. This plugin may require a newer version of the dprint CLI. Try running: dprint upgrade";

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
  /// Whether the plugin formats a file it matches in addition to the plugin
  /// that claims the file, rather than claiming the file itself.
  ///
  /// This is for plugins that do something to a file other than format it
  /// from scratch (ex. sorting the keys of a `package.json`), which would
  /// otherwise stop the file's usual formatter from running.
  #[serde(default)]
  pub additive: bool,
}

#[cfg(test)]
mod test {
  use super::*;

  fn plugin_info() -> PluginInfo {
    PluginInfo {
      name: "test".to_string(),
      version: "0.1.0".to_string(),
      config_key: "test".to_string(),
      help_url: "help".to_string(),
      config_schema_url: "schema".to_string(),
      update_url: None,
      supports_range_formatting: Some(true),
    }
  }

  #[test]
  fn plugin_info_message_has_flattened_info_and_capabilities() {
    let json = serde_json::to_value(PluginInfoMessage::current(plugin_info())).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "name": "test",
        "version": "0.1.0",
        "configKey": "test",
        "helpUrl": "help",
        "configSchemaUrl": "schema",
        "updateUrl": null,
        "supportsRangeFormatting": true,
        "capabilities": { "multiRangeFormat": true },
      })
    );
    assert_eq!(
      serde_json::from_value::<PluginInfoMessage>(json).unwrap(),
      PluginInfoMessage::current(plugin_info())
    );
  }

  #[test]
  fn plugin_info_message_without_capabilities_has_none() {
    // a plugin made before capabilities existed
    let message: PluginInfoMessage = serde_json::from_value(serde_json::to_value(plugin_info()).unwrap()).unwrap();
    assert_eq!(message.info, plugin_info());
    assert_eq!(message.capabilities, PluginCapabilities { multi_range_format: false });
    // an unknown capability is ignored
    let message: PluginInfoMessage = serde_json::from_str(
      r#"{"name":"test","version":"0.1.0","configKey":"test","helpUrl":"help","configSchemaUrl":"schema","capabilities":{"future":true}}"#,
    )
    .unwrap();
    assert_eq!(message.capabilities, PluginCapabilities::default());
  }
}
