use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;

use super::resolve_global_config_dir;
use super::resolve_global_config_dir_path;

/// Name of the file in the global config directory that has what the user
/// allowed the editor integrations to do.
const PERMISSIONS_FILE_NAME: &str = "permissions.json";

/// What the user allowed the editor integrations (the language server and the
/// editor service) to do.
///
/// IMPORTANT: This is only read from the global config directory and never from
/// something a project controls (ex. a config file or the editor's workspace
/// settings) because that would let a project allow itself.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
  /// Whether process plugins run without the user approving them, which is
  /// the default for now.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub allow_process_plugins: Option<bool>,
  /// The config files the user approved running the process plugins of.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

/// Reads what the user allowed, which is nothing when it was never saved or
/// can't be read.
pub fn read_permissions(environment: &impl Environment) -> Permissions {
  // nothing is created just to find out there's nothing in it
  let Some(file_path) = get_permissions_file_path(environment) else {
    return Default::default();
  };
  if !environment.path_exists(&file_path) {
    return Default::default();
  }
  let result = environment
    .read_file(&file_path)
    .map_err(anyhow::Error::from)
    .and_then(|text| Ok(serde_json::from_str::<Permissions>(&text)?));
  match result {
    Ok(permissions) => permissions,
    Err(err) => {
      log_warn!(
        environment,
        "Ignoring the permissions in {} because of a problem reading them: {:#}",
        file_path.display(),
        err
      );
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
  let mut permissions = read_permissions(environment);
  // the approval of a previous configuration isn't needed anymore
  permissions
    .approved_process_plugins
    .retain(|approved| approved.config_file != approval.config_file);
  permissions.approved_process_plugins.push(approval.clone());
  write_permissions(environment, &permissions)
}

/// Gets the path of the file the permissions are stored in.
pub fn get_permissions_file_path(environment: &impl Environment) -> Option<PathBuf> {
  resolve_global_config_dir_path(environment).map(|dir| dir.join(PERMISSIONS_FILE_NAME))
}

fn write_permissions(environment: &impl Environment, permissions: &Permissions) -> Result<PathBuf> {
  let dir = resolve_global_config_dir(environment).context("Could not find the global config directory.")?;
  let file_path = dir.join(PERMISSIONS_FILE_NAME);
  let mut text = serde_json::to_string_pretty(permissions)?;
  text.push('\n');
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
    environment
      .mk_dir_all(get_permissions_file_path(environment).unwrap().parent().unwrap())
      .unwrap();
  }

  #[test]
  fn should_allow_process_plugins_by_default() {
    let environment = TestEnvironment::new();
    assert!(read_permissions(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
  }

  #[test]
  fn should_add_approvals_replacing_the_one_for_a_previous_config() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    let file_path = get_permissions_file_path(&environment).unwrap();
    environment.write_file(&file_path, r#"{ "allowProcessPlugins": false }"#).unwrap();

    add_process_plugin_approval(&environment, &approval("/a/dprint.json", "1")).unwrap();
    add_process_plugin_approval(&environment, &approval("/b/dprint.json", "1")).unwrap();
    assert_eq!(add_process_plugin_approval(&environment, &approval("/a/dprint.json", "2")).unwrap(), file_path);

    let permissions = read_permissions(&environment);
    assert_eq!(permissions.allow_process_plugins, Some(false));
    assert_eq!(
      permissions.approved_process_plugins,
      vec![approval("/b/dprint.json", "1"), approval("/a/dprint.json", "2")]
    );
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "2")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/c/dprint.json", "1")));
    assert!(permissions.was_approved_with_other_config(&approval("/a/dprint.json", "3")));
    assert!(!permissions.was_approved_with_other_config(&approval("/c/dprint.json", "3")));
  }

  #[test]
  fn should_not_allow_process_plugins_when_permissions_are_invalid() {
    let environment = TestEnvironment::new();
    ensure_global_config_dir(&environment);
    environment.write_file(&get_permissions_file_path(&environment).unwrap(), "{").unwrap();
    assert!(!read_permissions(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert_eq!(environment.take_stderr_messages().len(), 1);
  }
}
