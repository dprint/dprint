use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;

use super::resolve_global_config_dir;
use super::resolve_global_config_dir_path;

/// Name of the file in the global config directory that `dprint init` keeps
/// what the user prefers in. It's only ever written by `dprint init`.
const INIT_PREFERENCES_FILE_NAME: &str = "init.json";

/// What the user told `dprint init` to remember for the next time it runs.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitPreferences {
  /// The plugins to select ahead of the others that format the same files, in
  /// the order they go in a config file's `plugins` array.
  #[serde(default)]
  pub preferred_plugins: Vec<PreferredPlugin>,
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
  // nothing is created just to find out there's nothing in it
  let Some(file_path) = resolve_global_config_dir_path(environment).map(|dir| dir.join(INIT_PREFERENCES_FILE_NAME)) else {
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

/// Writes what the user prefers, providing the path of the file it's in.
pub fn write_init_preferences(environment: &impl Environment, preferences: &InitPreferences) -> Result<PathBuf> {
  let file_path = resolve_init_preferences_path(environment).context("Could not find the global config directory.")?;
  if let Some(dir) = file_path.parent() {
    _ = environment.mk_dir_all(dir);
  }
  let mut text = serde_json::to_string_pretty(preferences)?;
  text.push('\n');
  environment
    .write_file(&file_path, &text)
    .with_context(|| format!("Failed writing {}", file_path.display()))?;
  Ok(file_path)
}

fn resolve_init_preferences_path(environment: &impl Environment) -> Option<PathBuf> {
  let dir = resolve_global_config_dir(environment).ok()?;
  Some(dir.join(INIT_PREFERENCES_FILE_NAME))
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
    };
    write_init_preferences(&environment, &preferences).unwrap();
    assert_eq!(
      environment.read_file("/config/dprint/init.json").unwrap(),
      r#"{
  "preferredPlugins": [
    {
      "name": "dprint-plugin-biome",
      "excludedFileExtensions": [
        "yaml"
      ],
      "config": {
        "indentStyle": "space"
      }
    },
    {
      "name": "dprint-plugin-json"
    }
  ]
}
"#
    );
    assert_eq!(read_init_preferences(&environment), preferences);
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
