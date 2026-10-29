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
  /// Whether to format again to ensure a stable format, which is what the
  /// `DPRINT_EDITOR_STABLE_FORMAT` environment variable says when `None`.
  pub ensure_stable_format: Option<bool>,
  /// Whether to log debug messages, which is the log level dprint was started
  /// with when `None`.
  pub verbose: Option<bool>,
}

impl LspSettings {
  /// Gets the settings to use until the client provides some.
  pub fn from_environment(environment: &impl Environment) -> Self {
    Self {
      use_global_config: use_global_config_env_var(environment),
      show_no_config_notification: true,
      ensure_stable_format: None,
      verbose: None,
    }
  }

  /// Updates the settings with the ones in the provided value, which may have
  /// them at the top level or within a `dprint` property. The settings that
  /// aren't in the value are left as-is and a `null` one is set back to its
  /// default.
  pub fn update(&mut self, value: &Value, environment: &impl Environment) {
    let value = value.get("dprint").filter(|value| value.is_object()).unwrap_or(value);
    let defaults = Self::from_environment(environment);
    match value.get("useGlobalConfig") {
      Some(Value::Bool(use_global_config)) => self.use_global_config = *use_global_config,
      Some(Value::Null) => self.use_global_config = defaults.use_global_config,
      _ => {}
    }
    match value.get("showNoConfigNotification") {
      Some(Value::Bool(show_no_config_notification)) => self.show_no_config_notification = *show_no_config_notification,
      Some(Value::Null) => self.show_no_config_notification = defaults.show_no_config_notification,
      _ => {}
    }
    match value.get("ensureStableFormat") {
      Some(Value::Bool(ensure_stable_format)) => self.ensure_stable_format = Some(*ensure_stable_format),
      Some(Value::Null) => self.ensure_stable_format = defaults.ensure_stable_format,
      _ => {}
    }
    match value.get("verbose") {
      Some(Value::Bool(verbose)) => self.verbose = Some(*verbose),
      Some(Value::Null) => self.verbose = defaults.verbose,
      _ => {}
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
    let environment = TestEnvironment::new();
    let mut settings = LspSettings {
      use_global_config: false,
      show_no_config_notification: true,
      ensure_stable_format: None,
      verbose: None,
    };

    // top level
    settings.update(&json!({ "useGlobalConfig": true }), &environment);
    assert_eq!(
      settings,
      LspSettings {
        use_global_config: true,
        show_no_config_notification: true,
        ensure_stable_format: None,
        verbose: None,
      }
    );

    // within a `dprint` property
    settings.update(
      &json!({ "dprint": { "useGlobalConfig": false, "showNoConfigNotification": false } }),
      &environment,
    );
    assert_eq!(
      settings,
      LspSettings {
        use_global_config: false,
        show_no_config_notification: false,
        ensure_stable_format: None,
        verbose: None,
      }
    );

    // ignores what's missing or not a boolean
    for value in [json!(null), json!({}), json!({ "dprint": null }), json!({ "useGlobalConfig": "true" })] {
      settings.update(&value, &environment);
      assert_eq!(
        settings,
        LspSettings {
          use_global_config: false,
          show_no_config_notification: false,
          ensure_stable_format: None,
          verbose: None,
        }
      );
    }
  }

  #[test]
  fn resets_settings_to_their_defaults_with_null() {
    let environment = TestEnvironment::new();
    environment.set_env_var("DPRINT_EDITOR_USE_GLOBAL_CONFIG", Some("1"));
    let mut settings = LspSettings::from_environment(&environment);
    settings.update(&json!({ "useGlobalConfig": false, "ensureStableFormat": true, "verbose": true }), &environment);
    assert!(!settings.use_global_config);
    settings.update(&json!({ "useGlobalConfig": null, "ensureStableFormat": null, "verbose": null }), &environment);
    assert_eq!(settings, LspSettings::from_environment(&environment));
    assert!(settings.use_global_config);
  }
}
