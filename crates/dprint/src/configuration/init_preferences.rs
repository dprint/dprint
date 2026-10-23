use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;

use super::Settings;
use super::read_settings;
use super::resolve_global_config_dir_path;
use super::set_setting;

/// Name of the file in the global config directory that `dprint init` kept
/// what the user prefers in before they were moved to the settings file.
const LEGACY_INIT_PREFERENCES_FILE_NAME: &str = "init.json";

/// What the user told `dprint init` to remember for the next time it runs.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitPreferences {
  /// The plugins to select ahead of the others that format the same files, in
  /// the order they go in a config file's `plugins` array.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub preferred_plugins: Vec<PreferredPlugin>,
  /// Names of the plugins to never pre-select, which are listed after the
  /// others. Preferring a plugin wins when it's in both.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub avoided_plugins: Vec<String>,
}

impl InitPreferences {
  pub fn is_preferred(&self, plugin_name: &str) -> bool {
    self.preferred_plugins.iter().any(|preferred| preferred.name == plugin_name)
  }

  pub fn is_avoided(&self, plugin_name: &str) -> bool {
    !self.is_preferred(plugin_name) && self.avoided_plugins.iter().any(|name| name == plugin_name)
  }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferredPlugin {
  /// The name of the plugin as it appears in the plugin info file.
  pub name: String,
  /// Lowercased file extensions the plugin matches that it shouldn't format.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub excluded_file_extensions: Vec<String>,
  /// File names the plugin matches that it shouldn't format.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub excluded_file_names: Vec<String>,
  /// The options to write to the plugin's config block.
  #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
  pub config: serde_json::Map<String, serde_json::Value>,
}

/// Reads what the user prefers, which is nothing when it was never saved or
/// can't be read. `dprint init` works without it, so a problem with the file
/// is only warned about.
pub fn read_init_preferences(environment: &impl Environment) -> InitPreferences {
  match read_settings(environment) {
    Ok(Some(Settings { init: Some(preferences), .. })) => preferences,
    // they're moved to the settings file the next time they're written
    Ok(_) => read_legacy_init_preferences(environment),
    Err(err) => {
      log_warn!(environment, "Ignoring the preferences because of a problem reading the settings: {:#}", err);
      Default::default()
    }
  }
}

/// Writes what the user prefers to the settings file, providing its path.
pub fn write_init_preferences(environment: &impl Environment, preferences: &InitPreferences) -> Result<PathBuf> {
  let file_path = set_setting(environment, &["init"], serde_json::to_value(preferences)?)?;
  // the preferences were moved to the settings file
  if let Some(legacy_file_path) = get_legacy_init_preferences_path(environment)
    && environment.path_exists(&legacy_file_path)
    && let Err(err) = environment.remove_file(&legacy_file_path)
  {
    log_debug!(environment, "Failed removing {}: {:#}", legacy_file_path.display(), err);
  }
  Ok(file_path)
}

/// Reads the preferences from the file they were in before they were in the
/// settings file (dprint 0.64).
fn read_legacy_init_preferences(environment: &impl Environment) -> InitPreferences {
  let Some(file_path) = get_legacy_init_preferences_path(environment) else {
    return Default::default();
  };
  if !environment.path_exists(&file_path) {
    return Default::default();
  }
  let result = environment
    .read_file(&file_path)
    .map_err(anyhow::Error::from)
    .and_then(|text| Ok(serde_json::from_str::<InitPreferences>(&text)?));
  match result {
    Ok(preferences) => preferences,
    Err(err) => {
      log_warn!(
        environment,
        "Ignoring the preferences in {} because of a problem reading them: {:#}",
        file_path.display(),
        err
      );
      Default::default()
    }
  }
}

fn get_legacy_init_preferences_path(environment: &impl Environment) -> Option<PathBuf> {
  resolve_global_config_dir_path(environment).map(|dir| dir.join(LEGACY_INIT_PREFERENCES_FILE_NAME))
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use pretty_assertions::assert_eq;

  #[test]
  fn should_round_trip_preferences() {
    let environment = TestEnvironment::new();
    // macOS only uses this directory when it's already there
    environment.mk_dir_all("/config/dprint").unwrap();
    assert_eq!(read_init_preferences(&environment), InitPreferences::default());

    let preferences = InitPreferences {
      preferred_plugins: vec![
        PreferredPlugin {
          name: "dprint-plugin-biome".to_string(),
          excluded_file_extensions: vec!["yaml".to_string()],
          excluded_file_names: vec![],
          config: serde_json::json!({ "indentStyle": "space" }).as_object().unwrap().clone(),
        },
        PreferredPlugin {
          name: "dprint-plugin-json".to_string(),
          ..Default::default()
        },
      ],
      avoided_plugins: vec!["dprint-plugin-prettier".to_string()],
    };
    write_init_preferences(&environment, &preferences).unwrap();
    assert_eq!(
      environment.read_file("/config/dprint/settings.json").unwrap(),
      r#"{
  "init": {
    "preferredPlugins": [
      {
        "name": "dprint-plugin-biome",
        "excludedFileExtensions": ["yaml"],
        "config": {
          "indentStyle": "space"
        }
      },
      {
        "name": "dprint-plugin-json"
      }
    ],
    "avoidedPlugins": ["dprint-plugin-prettier"]
  }
}
"#
    );
    assert_eq!(read_init_preferences(&environment), preferences);
  }

  #[test]
  fn should_move_preferences_from_init_json_to_the_settings_file() {
    // dprint 0.64 kept them in an init.json file
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/config/dprint").unwrap();
    environment
      .write_file("/config/dprint/settings.json", r#"{ "editor": { "allowProcessPlugins": false } }"#)
      .unwrap();
    environment
      .write_file("/config/dprint/init.json", r#"{ "avoidedPlugins": ["dprint-plugin-prettier"] }"#)
      .unwrap();
    let mut preferences = read_init_preferences(&environment);
    assert_eq!(preferences.avoided_plugins, vec!["dprint-plugin-prettier".to_string()]);

    preferences.avoided_plugins.push("dprint-plugin-biome".to_string());
    write_init_preferences(&environment, &preferences).unwrap();
    assert!(!environment.path_exists("/config/dprint/init.json"));
    assert_eq!(
      environment.read_file("/config/dprint/settings.json").unwrap(),
      r#"{
  "editor": { "allowProcessPlugins": false },
  "init": {
    "avoidedPlugins": ["dprint-plugin-prettier", "dprint-plugin-biome"]
  }
}
"#
    );
    assert_eq!(read_init_preferences(&environment), preferences);
  }

  #[test]
  fn should_prefer_a_plugin_that_is_also_avoided() {
    let preferences = InitPreferences {
      preferred_plugins: vec![PreferredPlugin {
        name: "a".to_string(),
        ..Default::default()
      }],
      avoided_plugins: vec!["a".to_string(), "b".to_string()],
    };
    assert!(preferences.is_preferred("a"));
    assert!(!preferences.is_avoided("a"));
    assert!(!preferences.is_preferred("b"));
    assert!(preferences.is_avoided("b"));
    assert!(!preferences.is_avoided("c"));
  }

  #[test]
  fn should_warn_and_ignore_preferences_that_do_not_parse() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/config/dprint").unwrap();
    environment.write_file("/config/dprint/init.json", "{ nope").unwrap();
    assert_eq!(read_init_preferences(&environment), InitPreferences::default());
    let messages = environment.take_stderr_messages();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Ignoring the preferences in"), "{}", messages[0]);
  }
}
