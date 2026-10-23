use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use jsonc_parser::cst::CstRootNode;
use serde::Deserialize;

use crate::environment::Environment;

use super::InitPreferences;
use super::get_init_config_file_text::to_cst_input;
use super::resolve_global_config_dir;
use super::resolve_global_config_dir_path;

/// Name of the file in the global config directory, next to the global config
/// file, that has the user's settings that aren't formatting configuration.
const SETTINGS_FILE_NAME: &str = "settings.json";

/// The user's settings that aren't formatting configuration.
///
/// IMPORTANT: This is only read from the global config directory and never from
/// something a project controls (ex. a config file or the editor's workspace
/// settings) because that would let a project allow itself to run process
/// plugins.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  /// The settings of the editor integrations (the language server and the
  /// editor service).
  #[serde(default)]
  pub editor: EditorSettings,
  /// What the user told `dprint init` to remember for the next time it runs.
  #[serde(default)]
  pub init: Option<InitPreferences>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
  /// Whether process plugins run without the user approving them, which is
  /// the default for now.
  #[serde(default)]
  pub allow_process_plugins: Option<bool>,
}

/// Gets the path of the settings file.
pub fn get_settings_file_path(environment: &impl Environment) -> Option<PathBuf> {
  resolve_global_config_dir_path(environment).map(|dir| dir.join(SETTINGS_FILE_NAME))
}

/// Reads the settings, which are `None` when there's no settings file.
pub(super) fn read_settings(environment: &impl Environment) -> Result<Option<Settings>> {
  // nothing is created just to find out there's nothing in it
  let Some(file_path) = get_settings_file_path(environment) else {
    return Ok(None);
  };
  if !environment.path_exists(&file_path) {
    return Ok(None);
  }
  let text = environment
    .read_file(&file_path)
    .with_context(|| format!("Failed reading {}", file_path.display()))?;
  jsonc_parser::parse_to_serde_value::<Option<Settings>>(&text, &Default::default()).with_context(|| format!("Failed parsing {}", file_path.display()))
}

/// Sets a property of the settings at the path of property names, keeping the
/// rest of the file as it is (ex. its comments) because people edit it too.
/// Provides the path of the file.
pub(super) fn set_setting(environment: &impl Environment, property_path: &[&str], value: serde_json::Value) -> Result<PathBuf> {
  let dir = resolve_global_config_dir(environment).context("Could not find the global config directory.")?;
  _ = environment.mk_dir_all(&dir);
  let file_path = dir.join(SETTINGS_FILE_NAME);
  let text = if environment.path_exists(&file_path) {
    environment
      .read_file(&file_path)
      .with_context(|| format!("Failed reading {}", file_path.display()))?
  } else {
    String::new()
  };
  let root = CstRootNode::parse(&text, &Default::default()).with_context(|| format!("Failed parsing {}", file_path.display()))?;
  let (name, parent_names) = property_path.split_last().expect("property path should not be empty");
  let mut obj = root.object_value_or_set();
  for parent_name in parent_names {
    obj = obj.object_value_or_set(parent_name);
  }
  let value = to_cst_input(value);
  match obj.get(name) {
    Some(prop) => prop.set_value(value),
    None => {
      obj.append(name, value);
    }
  }
  let mut text = root.to_string();
  if !text.ends_with('\n') {
    text.push('\n');
  }
  environment
    .write_file(&file_path, &text)
    .with_context(|| format!("Failed writing {}", file_path.display()))?;
  Ok(file_path)
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use pretty_assertions::assert_eq;

  fn ensure_global_config_dir(environment: &TestEnvironment) {
    // macOS only uses this directory when it's already there
    environment.mk_dir_all(get_settings_file_path(environment).unwrap().parent().unwrap()).unwrap();
  }

  #[test]
  fn should_set_settings_keeping_the_rest_of_the_file() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = get_settings_file_path(&environment).unwrap();
    environment
      .write_file(
        &file_path,
        r#"{
  "editor": {
    // ask before running process plugins
    "allowProcessPlugins": false
  },
  "other": 1
}
"#,
      )
      .unwrap();

    set_setting(&environment, &["editor", "value"], serde_json::json!([1, 2])).unwrap();
    assert_eq!(set_setting(&environment, &["init"], serde_json::json!({ "a": true })).unwrap(), file_path);
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "editor": {
    // ask before running process plugins
    "allowProcessPlugins": false,
    "value": [1, 2]
  },
  "other": 1,
  "init": {
    "a": true
  }
}
"#
    );
    assert_eq!(read_settings(&environment).unwrap().unwrap().editor.allow_process_plugins, Some(false));
  }

  #[test]
  fn should_create_settings_file() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = set_setting(&environment, &["editor", "allowProcessPlugins"], serde_json::json!(false)).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "editor": {
    "allowProcessPlugins": false
  }
}
"#
    );
  }

  #[test]
  fn should_not_replace_settings_that_do_not_parse() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.write_file(&file_path, "{ nope").unwrap();
    assert!(read_settings(&environment).is_err());
    assert!(set_setting(&environment, &["init"], serde_json::json!({})).is_err());
    assert_eq!(environment.read_file(&file_path).unwrap(), "{ nope");
  }
}
