use serde_json::Value;

use crate::environment::Environment;

/// The settings a client provides in the `initializationOptions` of the
/// initialize request and in `workspace/didChangeConfiguration` notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspSettings {
  /// Whether to format the files that don't have a config file in an
  /// ancestor directory using the global config file.
  pub use_global_config: bool,
  /// Whether to notify the first time in a session that a file isn't
  /// formatted because no config file was found for it.
  pub show_no_config_notification: bool,
}

impl LspSettings {
  /// Gets the settings to use until the client provides some.
  pub fn from_environment(environment: &impl Environment) -> Self {
    Self {
      use_global_config: use_global_config_env_var(environment),
      show_no_config_notification: true,
    }
  }

  /// Updates the settings with the ones in the provided value, which may have
  /// them at the top level or within a `dprint` property. The settings that
  /// aren't in the value are left as-is.
  pub fn update(&mut self, value: &Value) {
    let value = value.get("dprint").filter(|value| value.is_object()).unwrap_or(value);
    if let Some(use_global_config) = value.get("useGlobalConfig").and_then(|value| value.as_bool()) {
      self.use_global_config = use_global_config;
    }
    if let Some(show_no_config_notification) = value.get("showNoConfigNotification").and_then(|value| value.as_bool()) {
      self.show_no_config_notification = show_no_config_notification;
    }
  }
}

/// An editor that can't provide settings to the server may opt into using the
/// global config file by setting the `DPRINT_EDITOR_USE_GLOBAL_CONFIG`
/// environment variable to `1` or `true`.
fn use_global_config_env_var(environment: &impl Environment) -> bool {
  environment.env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG").is_some_and(|value| {
    let value = value.to_string_lossy();
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true")
  })
}

#[cfg(test)]
mod test {
  use serde_json::json;

  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn use_global_config_from_env_var() {
    let environment = TestEnvironment::new();
    assert!(!LspSettings::from_environment(&environment).use_global_config);
    for (value, expected) in [
      ("1", true),
      ("true", true),
      ("TRUE", true),
      (" 1\n", true),
      ("0", false),
      ("false", false),
      ("", false),
    ] {
      environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some(value));
      assert_eq!(LspSettings::from_environment(&environment).use_global_config, expected, "{:?}", value);
    }
  }

  #[test]
  fn updates_settings() {
    let mut settings = LspSettings {
      use_global_config: false,
      show_no_config_notification: true,
    };

    // top level
    settings.update(&json!({ "useGlobalConfig": true }));
    assert_eq!(
      settings,
      LspSettings {
        use_global_config: true,
        show_no_config_notification: true,
      }
    );

    // within a `dprint` property
    settings.update(&json!({ "dprint": { "useGlobalConfig": false, "showNoConfigNotification": false } }));
    assert_eq!(
      settings,
      LspSettings {
        use_global_config: false,
        show_no_config_notification: false,
      }
    );

    // ignores what's missing or not a boolean
    for value in [json!(null), json!({}), json!({ "dprint": null }), json!({ "useGlobalConfig": "true" })] {
      settings.update(&value);
      assert_eq!(
        settings,
        LspSettings {
          use_global_config: false,
          show_no_config_notification: false,
        }
      );
    }
  }
}
