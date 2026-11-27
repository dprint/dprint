use serde::Deserialize;
use serde::Serialize;

/// Information about a plugin, as the CLI receives it from a Wasm plugin
/// ([`dprint_core::plugins::PluginInfo`]) or a process plugin
/// ([`dprint_core::plugins::ProcessPluginInfo`]).
///
/// Plugins made with an older version of dprint-core might not provide some of
/// the properties, so those are optional here.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedPluginInfo {
  pub name: String,
  pub version: String,
  pub config_key: String,
  pub help_url: String,
  pub config_schema_url: String,
  pub update_url: Option<String>,
  /// Whether the plugin formats the range of a format request.
  ///
  /// - `Some(true)` - Editors give the plugin the range as it is and use what
  ///   it formats, so it decides what to format around the range (ex. the
  ///   nodes it overlaps).
  /// - `Some(false)` - Editors don't format a range with the plugin.
  /// - `None` - Only for plugins made before this existed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub supports_range_formatting: Option<bool>,
  /// Whether the plugin says formatting its output again never changes it,
  /// so dprint doesn't need to format a file it changed again until it's
  /// stable. Plugins made before this existed don't provide it, so they get
  /// formatted until the output is stable.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub skip_stable_format: bool,
  /// Whether another plugin may change this process plugin's non-global
  /// configuration when it asks dprint to format text with this plugin.
  ///
  /// Wasm plugins don't provide this because they're sandboxed. For a process
  /// plugin, `None` is the same as `Some(false)`, so plugins made before this
  /// existed (ex. older versions of dprint-plugin-exec) stay protected.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub allow_plugin_override_config: Option<bool>,
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_not_skip_stable_format_for_plugins_that_do_not_say() {
    let json = r#"{"name":"a","version":"1.0.0","configKey":"a","helpUrl":"","configSchemaUrl":"","updateUrl":null}"#;
    let info: ReceivedPluginInfo = serde_json::from_str(json).unwrap();
    assert!(!info.skip_stable_format);
    // it's not written when false
    assert!(!serde_json::to_string(&info).unwrap().contains("skipStableFormat"));

    let json = r#"{"name":"a","version":"1.0.0","configKey":"a","helpUrl":"","configSchemaUrl":"","updateUrl":null,"skipStableFormat":true}"#;
    let info: ReceivedPluginInfo = serde_json::from_str(json).unwrap();
    assert!(info.skip_stable_format);
    assert!(serde_json::to_string(&info).unwrap().contains(r#""skipStableFormat":true"#));
  }
}
