use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use anyhow::bail;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;

/// A permission value in the deno permission model.
///
/// - `Boolean(true)` means allow all (e.g., `--allow-env`)
/// - `Boolean(false)` means deny (omit the flag)
/// - `Scoped(vec)` means scoped access (e.g., `--allow-read=.,/tmp`)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DenoPermissionValue {
  Boolean(bool),
  Scoped(Vec<String>),
}

/// Permissions for a deno plugin.
///
/// Runtime permission keys: `env`, `read`, `write`, `net`, `run`, `ffi`, `sys`.
/// Install-time key: `allowScripts` (npm packages needing lifecycle scripts).
pub type DenoPermissions = BTreeMap<String, DenoPermissionValue>;

const RUNTIME_PERMISSION_KEYS: &[&str] = &["env", "read", "write", "net", "run", "ffi", "sys"];

/// Converts structured permissions to deno CLI args.
pub fn permissions_to_deno_args(permissions: &DenoPermissions) -> Vec<String> {
  let mut args = Vec::new();
  for key in RUNTIME_PERMISSION_KEYS {
    if let Some(value) = permissions.get(*key) {
      match value {
        DenoPermissionValue::Boolean(true) => {
          args.push(format!("--allow-{}", key));
        }
        DenoPermissionValue::Scoped(scopes) if !scopes.is_empty() => {
          args.push(format!("--allow-{}={}", key, scopes.join(",")));
        }
        _ => {}
      }
    }
  }
  args
}

/// Returns the default deno permissions (used when neither manifest nor user config specifies any).
pub fn default_deno_permissions() -> DenoPermissions {
  let mut permissions = BTreeMap::new();
  permissions.insert("env".to_string(), DenoPermissionValue::Boolean(true));
  permissions.insert("read".to_string(), DenoPermissionValue::Boolean(true));
  permissions
}

/// Validates that all permissions required by the manifest are granted by the user config.
pub fn validate_permissions(required: &DenoPermissions, granted: &DenoPermissions) -> Result<()> {
  for (key, required_value) in required {
    if key == "allowScripts" {
      // validate allowScripts separately
      if let DenoPermissionValue::Scoped(required_scripts) = required_value {
        match granted.get("allowScripts") {
          Some(DenoPermissionValue::Scoped(granted_scripts)) => {
            for script in required_scripts {
              if !granted_scripts.contains(script) {
                bail!(
                  "Plugin requires allowScripts for '{}', but it was not granted in the plugin's config. \
                   Add it to the \"permissions\" in the plugin's configuration section.",
                  script
                );
              }
            }
          }
          _ => {
            bail!(
              "Plugin requires allowScripts for [{}], but none were granted in the plugin's config. \
               Add \"allowScripts\": [{}] to the \"permissions\" in the plugin's configuration section.",
              required_scripts.join(", "),
              required_scripts.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")
            );
          }
        }
      }
      continue;
    }

    match granted.get(key) {
      Some(_) => {} // permission granted (we don't validate scope subsetting for now)
      None => {
        bail!(
          "Plugin requires '{}' permission, but it was not granted in the plugin's config. \
           Add it to the \"permissions\" in the plugin's configuration section.",
          key
        );
      }
    }
  }
  Ok(())
}

/// Extracts the `permissions` key from a plugin config map, removing it so it
/// doesn't get sent to the plugin as configuration.
pub fn extract_permissions_from_config(config: &mut ConfigKeyMap) -> Option<DenoPermissions> {
  let value = config.shift_remove("permissions")?;
  match value {
    ConfigKeyValue::Object(map) => {
      let mut permissions = BTreeMap::new();
      for (key, val) in map {
        match val {
          ConfigKeyValue::Bool(b) => {
            permissions.insert(key, DenoPermissionValue::Boolean(b));
          }
          ConfigKeyValue::Array(arr) => {
            let scopes: Vec<String> = arr
              .into_iter()
              .filter_map(|v| match v {
                ConfigKeyValue::String(s) => Some(s),
                _ => None,
              })
              .collect();
            permissions.insert(key, DenoPermissionValue::Scoped(scopes));
          }
          _ => {}
        }
      }
      Some(permissions)
    }
    _ => None,
  }
}

/// Resolves the deno executable path.
///
/// Checks `DPRINT_DENO_PATH` env var first, then falls back to `"deno"` on PATH.
pub fn resolve_deno_executable(environment: &impl Environment) -> Result<PathBuf> {
  if let Some(path) = environment.env_var("DPRINT_DENO_PATH") {
    let path = PathBuf::from(path);
    if !environment.path_exists(&path) {
      bail!("DPRINT_DENO_PATH is set to '{}', but the file does not exist.", path.display());
    }
    return Ok(path);
  }
  // fall back to "deno" and rely on PATH resolution
  Ok(PathBuf::from("deno"))
}

/// Extracts the `allowScripts` list from permissions (for running `deno install`).
pub fn get_allow_scripts(permissions: &DenoPermissions) -> Option<Vec<String>> {
  match permissions.get("allowScripts")? {
    DenoPermissionValue::Scoped(scripts) if !scripts.is_empty() => Some(scripts.clone()),
    _ => None,
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_permissions_to_deno_args() {
    let mut perms = BTreeMap::new();
    perms.insert("env".to_string(), DenoPermissionValue::Boolean(true));
    perms.insert("read".to_string(), DenoPermissionValue::Scoped(vec![".".to_string()]));
    perms.insert("write".to_string(), DenoPermissionValue::Boolean(false));
    perms.insert(
      "allowScripts".to_string(),
      DenoPermissionValue::Scoped(vec!["npm:esbuild".to_string()]),
    );

    let args = permissions_to_deno_args(&perms);
    assert_eq!(args, vec!["--allow-env", "--allow-read=."]);
  }

  #[test]
  fn test_default_permissions() {
    let perms = default_deno_permissions();
    let args = permissions_to_deno_args(&perms);
    assert_eq!(args, vec!["--allow-env", "--allow-read"]);
  }

  #[test]
  fn test_validate_permissions_ok() {
    let mut required = BTreeMap::new();
    required.insert("env".to_string(), DenoPermissionValue::Boolean(true));
    required.insert("read".to_string(), DenoPermissionValue::Boolean(true));

    let mut granted = BTreeMap::new();
    granted.insert("env".to_string(), DenoPermissionValue::Boolean(true));
    granted.insert("read".to_string(), DenoPermissionValue::Scoped(vec![".".to_string()]));

    assert!(validate_permissions(&required, &granted).is_ok());
  }

  #[test]
  fn test_validate_permissions_missing() {
    let mut required = BTreeMap::new();
    required.insert("env".to_string(), DenoPermissionValue::Boolean(true));
    required.insert("net".to_string(), DenoPermissionValue::Boolean(true));

    let mut granted = BTreeMap::new();
    granted.insert("env".to_string(), DenoPermissionValue::Boolean(true));

    let err = validate_permissions(&required, &granted).unwrap_err();
    assert!(err.to_string().contains("'net' permission"), "{}", err);
  }

  #[test]
  fn test_validate_allow_scripts_missing() {
    let mut required = BTreeMap::new();
    required.insert(
      "allowScripts".to_string(),
      DenoPermissionValue::Scoped(vec!["npm:esbuild".to_string()]),
    );

    let granted = BTreeMap::new();
    let err = validate_permissions(&required, &granted).unwrap_err();
    assert!(err.to_string().contains("allowScripts"), "{}", err);
  }
}
