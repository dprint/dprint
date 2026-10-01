use deno_tower_lsp::lsp_types::ClientCapabilities;
use deno_tower_lsp::lsp_types::ConfigurationItem;
use deno_tower_lsp::lsp_types::Registration;
use serde::Deserialize;
use serde_json::Value;

/// The section of the client's settings that has the settings for dprint
/// (ex. the `dprint.ensureStableFormat` setting in vscode).
const SETTINGS_SECTION: &str = "dprint";

/// Settings the server gets from the client, which it gets itself so that a
/// client doesn't need to provide them when starting the server or restart
/// the server when they change.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LspSettings {
  /// Whether to format a file again until the output stops changing. Defaults
  /// to the `DPRINT_EDITOR_STABLE_FORMAT` environment variable.
  pub ensure_stable_format: Option<bool>,
  /// Whether to format files that don't have a config file in an ancestor
  /// directory using the global config file. Defaults to true.
  pub use_global_config: Option<bool>,
}

impl LspSettings {
  /// Gets the settings from the client's response to the configuration items.
  pub fn from_configuration(mut values: Vec<Value>) -> Self {
    if values.is_empty() {
      return Self::default();
    }
    // the client responds with null when it doesn't have the section
    serde_json::from_value(values.swap_remove(0)).unwrap_or_default()
  }
}

/// Whether the client supports the server requesting its settings.
pub fn supports_configuration(capabilities: &ClientCapabilities) -> bool {
  capabilities.workspace.as_ref().and_then(|w| w.configuration) == Some(true)
}

pub fn get_configuration_items() -> Vec<ConfigurationItem> {
  vec![ConfigurationItem {
    scope_uri: None,
    section: Some(SETTINGS_SECTION.to_string()),
  }]
}

/// Gets the registration that has the client tell the server when its settings
/// change, which is when the server gets them from the client again.
pub fn get_settings_change_registration(capabilities: &ClientCapabilities) -> Option<Registration> {
  if !supports_configuration(capabilities) {
    return None;
  }
  let did_change_configuration = capabilities.workspace.as_ref()?.did_change_configuration.as_ref()?;
  if did_change_configuration.dynamic_registration != Some(true) {
    return None;
  }
  Some(Registration {
    id: "dprint-settings".to_string(),
    method: "workspace/didChangeConfiguration".to_string(),
    register_options: Some(serde_json::json!({ "section": SETTINGS_SECTION })),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_settings_from_configuration() {
    fn get(value: Value) -> LspSettings {
      LspSettings::from_configuration(vec![value])
    }

    assert_eq!(
      get(serde_json::json!({
        "ensureStableFormat": true,
        "useGlobalConfig": false,
        "path": null,
      })),
      LspSettings {
        ensure_stable_format: Some(true),
        use_global_config: Some(false),
      }
    );
    assert_eq!(
      get(serde_json::json!({ "ensureStableFormat": null })),
      LspSettings {
        ensure_stable_format: None,
        use_global_config: None,
      }
    );
    assert_eq!(get(Value::Null), LspSettings::default());
    assert_eq!(LspSettings::from_configuration(Vec::new()), LspSettings::default());
  }

  #[test]
  fn test_get_settings_change_registration() {
    fn get(capabilities: Value) -> Option<Registration> {
      get_settings_change_registration(&serde_json::from_value(capabilities).unwrap())
    }

    let registration = get(serde_json::json!({
      "workspace": {
        "configuration": true,
        "didChangeConfiguration": { "dynamicRegistration": true },
      }
    }))
    .unwrap();
    assert_eq!(registration.method, "workspace/didChangeConfiguration");
    assert_eq!(registration.register_options, Some(serde_json::json!({ "section": "dprint" })));
    // can't get the settings
    assert_eq!(
      get(serde_json::json!({
        "workspace": {
          "didChangeConfiguration": { "dynamicRegistration": true },
        }
      })),
      None
    );
    // can't register
    assert_eq!(get(serde_json::json!({ "workspace": { "configuration": true } })), None);
    assert_eq!(get(serde_json::json!({})), None);
  }
}
