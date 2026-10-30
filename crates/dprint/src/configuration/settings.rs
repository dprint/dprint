use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use jsonc_parser::cst::CstRootNode;
use serde::Deserialize;

use crate::environment::Environment;
use crate::utils::TrustedDomains;

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
///
/// Every version of dprint that's installed reads the same file (ex. a
/// project's npm dprint next to a global one), so it changes by these rules:
///
/// - The editors don't use `editor` when it has a property or a value that the
///   version doesn't know (ex. an `editor.trustedDomains` entry that isn't a
///   domain). They only trust the default domains and ask before running
///   process plugins instead, like when the file can't be read, and say why in
///   the messages that ask the user (see `Settings::get_editor_problem`).
/// - So a setting that narrows what the editors allow goes in `editor`, as a
///   property or as a value of one that an older version rejects (ex. a
///   syntax for not trusting a default domain in `editor.trustedDomains`).
/// - A setting that doesn't narrow anything goes outside of `editor`, because
///   any new property in it has the older versions ignore
///   `editor.allowProcessPlugins` and `editor.trustedDomains`. A property
///   outside of `editor` that a version doesn't know is only logged.
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
  /// The paths of the properties dprint doesn't know (ex. `editor.allowProcessPlugin`),
  /// which are likely a typo.
  #[serde(skip)]
  pub unknown_properties: Vec<String>,
}

impl Settings {
  /// Gets why the editors don't use `editor`, which is when it has a property
  /// or a value that this version of dprint doesn't know, and is `None` when
  /// they use it. The editors treat that like settings that can't be read
  /// because it may be a setting of a newer version of dprint that narrows
  /// what's allowed (ex. trusting fewer domains). A misspelled property or a
  /// value that isn't a domain is treated that way too.
  ///
  /// It's said after the path of the file (ex. "has a property ...").
  pub fn get_editor_problem(&self) -> Option<String> {
    if let Some(name) = self.unknown_properties.iter().find(|name| name.starts_with("editor.")) {
      return Some(format!("has a property in \"editor\" that this version of dprint doesn't know ({})", name));
    }
    if let Some(domain) = self.editor.trusted_domains.iter().find(|domain| TrustedDomains::parse_domain(domain).is_none()) {
      return Some(format!("has a value in \"editor.trustedDomains\" that isn't a domain (\"{}\")", domain));
    }
    None
  }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
  /// Whether process plugins run without the user approving them.
  #[serde(default)]
  pub allow_process_plugins: Option<bool>,
  /// The domains the editors download from without asking the user in
  /// addition to the default ones (ex. a private npm registry), which are each
  /// a host name or an IP address (see `TrustedDomains::parse_domain`).
  #[serde(default)]
  pub trusted_domains: Vec<String>,
}

/// Gets the path of the settings file.
pub fn get_settings_file_path(environment: &impl Environment) -> Option<PathBuf> {
  resolve_global_config_dir_path(environment).map(|dir| dir.join(SETTINGS_FILE_NAME))
}

/// Reads the settings, which are `None` when there's no settings file.
pub fn read_settings(environment: &impl Environment) -> Result<Option<Settings>> {
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
  let Some(value) = jsonc_parser::parse_to_serde_value::<Option<serde_json::Value>>(&text, &Default::default())
    .with_context(|| format!("Failed parsing {}", file_path.display()))?
  else {
    return Ok(None);
  };
  let unknown_properties = get_unknown_properties(&value);
  let mut settings = serde_json::from_value::<Settings>(value).with_context(|| format!("Failed parsing {}", file_path.display()))?;
  settings.unknown_properties = unknown_properties;
  Ok(Some(settings))
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

/// Gets the paths of the properties of the settings that dprint doesn't know.
/// The `init` preferences aren't checked because only `dprint init` writes them.
fn get_unknown_properties(value: &serde_json::Value) -> Vec<String> {
  let mut unknown_properties = Vec::new();
  let Some(obj) = value.as_object() else {
    return unknown_properties;
  };
  for (name, value) in obj {
    match name.as_str() {
      "$schema" | "init" => {}
      "editor" => {
        if let Some(editor) = value.as_object() {
          for name in editor.keys() {
            if name != "allowProcessPlugins" && name != "trustedDomains" {
              unknown_properties.push(format!("editor.{}", name));
            }
          }
        }
      }
      _ => unknown_properties.push(name.clone()),
    }
  }
  unknown_properties
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
