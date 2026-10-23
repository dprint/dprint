use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use jsonc_parser::cst::CstRootNode;
use serde::Deserialize;
use serde::Serialize;

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
  /// The config files the user approved running the process plugins of.
  #[serde(default)]
  pub approved_process_plugins: Vec<ProcessPluginApproval>,
}

/// What the user allowed the editor integrations to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Permissions {
  pub allow_process_plugins: Option<bool>,
  pub approved_process_plugins: Vec<ProcessPluginApproval>,
}

impl Permissions {
  pub fn is_process_plugin_allowed(&self, approval: &ProcessPluginApproval) -> bool {
    // todo(dsherret): default to asking in a future version
    self.allow_process_plugins.unwrap_or(true) || self.approved_process_plugins.contains(approval)
  }

  /// Gets whether the process plugins of the config file were approved when
  /// it had a different configuration.
  pub fn was_approved_with_other_config(&self, approval: &ProcessPluginApproval) -> bool {
    self
      .approved_process_plugins
      .iter()
      .any(|approved| approved.config_file == approval.config_file && approved.config_hash != approval.config_hash)
  }
}

/// A config file that the user approved running the process plugins of.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginApproval {
  /// The config file that specifies the process plugins.
  pub config_file: String,
  /// The SHA-256 checksum of the config file's resolved plugins and
  /// configuration, so a change to it (ex. the commands of
  /// dprint-plugin-exec) needs to be approved again.
  pub config_hash: String,
}

/// Reads what the user allowed the editor integrations to do.
pub fn read_permissions(environment: &impl Environment) -> Permissions {
  match read_settings(environment) {
    Ok(settings) => {
      let settings = settings.unwrap_or_default().editor;
      Permissions {
        allow_process_plugins: settings.allow_process_plugins,
        approved_process_plugins: settings.approved_process_plugins,
      }
    }
    Err(err) => {
      log_warn!(environment, "Ignoring the settings because of a problem reading them: {:#}", err);
      // IMPORTANT: Don't allow what the user may have tried to disallow.
      Permissions {
        allow_process_plugins: Some(false),
        approved_process_plugins: Vec::new(),
      }
    }
  }
}

/// Stores that the user approved running the process plugins of the config
/// file, providing the path of the file it's in.
pub fn add_process_plugin_approval(environment: &impl Environment, approval: &ProcessPluginApproval) -> Result<PathBuf> {
  // fail instead of replacing the settings when they can't be read
  let mut approvals = read_settings(environment)?.unwrap_or_default().editor.approved_process_plugins;
  // the approval of a previous configuration isn't needed anymore
  approvals.retain(|approved| approved.config_file != approval.config_file);
  approvals.push(approval.clone());
  set_setting(environment, &["editor", "approvedProcessPlugins"], serde_json::to_value(&approvals)?)
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

  fn approval(config_file: &str, config_hash: &str) -> ProcessPluginApproval {
    ProcessPluginApproval {
      config_file: config_file.to_string(),
      config_hash: config_hash.to_string(),
    }
  }

  fn ensure_global_config_dir(environment: &TestEnvironment) {
    // macOS only uses this directory when it's already there
    environment.mk_dir_all(get_settings_file_path(environment).unwrap().parent().unwrap()).unwrap();
  }

  #[test]
  fn should_allow_process_plugins_by_default() {
    let environment = TestEnvironment::new();
    assert!(read_permissions(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
  }

  #[test]
  fn should_add_approvals_keeping_the_rest_of_the_file() {
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

    add_process_plugin_approval(&environment, &approval("/a/dprint.json", "1")).unwrap();
    add_process_plugin_approval(&environment, &approval("/b/dprint.json", "1")).unwrap();
    assert_eq!(add_process_plugin_approval(&environment, &approval("/a/dprint.json", "2")).unwrap(), file_path);

    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "editor": {
    // ask before running process plugins
    "allowProcessPlugins": false,
    "approvedProcessPlugins": [
      {
        "configFile": "/b/dprint.json",
        "configHash": "1"
      },
      {
        "configFile": "/a/dprint.json",
        "configHash": "2"
      }
    ]
  },
  "other": 1
}
"#
    );
    let permissions = read_permissions(&environment);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "2")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/c/dprint.json", "1")));
    assert!(permissions.was_approved_with_other_config(&approval("/a/dprint.json", "3")));
    assert!(!permissions.was_approved_with_other_config(&approval("/c/dprint.json", "3")));
  }

  #[test]
  fn should_create_settings_file() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = add_process_plugin_approval(&environment, &approval("/a/dprint.json", "1")).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "editor": {
    "approvedProcessPlugins": [
      {
        "configFile": "/a/dprint.json",
        "configHash": "1"
      }
    ]
  }
}
"#
    );
  }

  #[test]
  fn should_not_allow_process_plugins_or_replace_settings_that_do_not_parse() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.write_file(&file_path, "{ nope").unwrap();
    assert!(!read_permissions(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    let messages = environment.take_stderr_messages();
    assert_eq!(messages.len(), 1);
    assert!(
      messages[0].contains("Ignoring the settings because of a problem reading them"),
      "{}",
      messages[0]
    );
    assert!(add_process_plugin_approval(&environment, &approval("/a/dprint.json", "1")).is_err());
    assert_eq!(environment.read_file(&file_path).unwrap(), "{ nope");
  }
}
