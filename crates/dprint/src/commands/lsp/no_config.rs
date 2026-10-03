use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::configuration::resolve_global_config_dir;
use crate::environment::Environment;
use crate::utils::LaxSingleProcessFsFlag;

pub const DISMISS_ACTION_TITLE: &str = "Don't show again";
pub const DISMISS_WORKSPACE_ACTION_TITLE: &str = "Don't show in this workspace";

pub struct NoConfigMessageOptions {
  /// Whether the global config file would have been used.
  pub use_global_config: bool,
  /// Whether there's a global config file that isn't being used.
  pub has_global_config: bool,
}

/// Gets the message to show when no config file was found for a file.
pub fn get_no_config_message(options: NoConfigMessageOptions) -> String {
  let message = "No dprint configuration file found. Run \"dprint init\" in your project to create one";
  if options.use_global_config {
    format!("{} or \"dprint init --global\" to create a global one.", message)
  } else if options.has_global_config {
    format!(
      "{} or enable the \"useGlobalConfig\" setting of the dprint language server to use your global one.",
      message
    )
  } else {
    format!("{}.", message)
  }
}

/// Where the user asked to not be notified about a missing config file anymore.
pub enum NoConfigNotificationDismissal {
  Everywhere,
  WorkspaceFolders(Vec<PathBuf>),
}

/// Gets if the user dismissed the notification everywhere or for one of the
/// provided workspace folders.
pub fn is_no_config_notification_dismissed(environment: &impl Environment, workspace_folders: &[PathBuf]) -> bool {
  let state = read_state(environment).no_config_notification;
  state.dismissed
    || workspace_folders
      .iter()
      .any(|folder| state.dismissed_workspace_folders.contains(&folder_key(folder)))
}

/// Stores that the user dismissed the notification. A server can't update the
/// editor's settings, so this is stored in the global config directory instead.
pub async fn dismiss_no_config_notification<TEnvironment: Environment>(environment: &TEnvironment, dismissal: NoConfigNotificationDismissal) -> Result<()> {
  // prevent another language server process from storing a dismissal between
  // the read and write of the state file, which would lose one of the two
  let lock_dir = environment.get_cache_dir().join("locks");
  let _ignore = environment.mk_dir_all(&lock_dir);
  let _flag = LaxSingleProcessFsFlag::lock(
    environment,
    lock_dir.join(".lsp-state.lock"),
    "Waiting for file lock for the language server state...",
  )
  .await;
  store_dismissal(environment, dismissal)
}

fn store_dismissal(environment: &impl Environment, dismissal: NoConfigNotificationDismissal) -> Result<()> {
  let mut state = read_state(environment);
  match dismissal {
    NoConfigNotificationDismissal::Everywhere => state.no_config_notification.dismissed = true,
    NoConfigNotificationDismissal::WorkspaceFolders(folders) => {
      state
        .no_config_notification
        .dismissed_workspace_folders
        .extend(folders.iter().map(|folder| folder_key(folder)));
    }
  }
  let file_path = get_state_file_path(environment)?;
  // the directory doesn't exist until a global config file is created
  if let Some(dir_path) = file_path.parent() {
    environment.mk_dir_all(dir_path)?;
  }
  environment.atomic_write_file_bytes(file_path, &serde_json::to_vec_pretty(&state)?)?;
  Ok(())
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LspState {
  no_config_notification: NoConfigNotificationState,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct NoConfigNotificationState {
  dismissed: bool,
  dismissed_workspace_folders: BTreeSet<String>,
}

fn read_state(environment: &impl Environment) -> LspState {
  // start over when the file can't be read or was written by a version with another format
  get_state_file_path(environment)
    .ok()
    .and_then(|file_path| environment.maybe_read_file(file_path).ok().flatten())
    .and_then(|text| serde_json::from_str(&text).ok())
    .unwrap_or_default()
}

/// The file is beside the global config file rather than in the cache
/// directory so that clearing the cache doesn't bring the notification back.
fn get_state_file_path(environment: &impl Environment) -> Result<PathBuf> {
  Ok(resolve_global_config_dir(environment)?.join("lsp-state.json"))
}

fn folder_key(folder: &Path) -> String {
  folder.to_string_lossy().into_owned()
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn no_config_message() {
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: true,
        has_global_config: false,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one or \"dprint init --global\" to create a global one."
    );
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: false,
        has_global_config: true,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one or enable the \"useGlobalConfig\" setting of the dprint language server to use your global one."
    );
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: false,
        has_global_config: false,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one."
    );
  }

  #[test]
  fn dismisses_for_workspace_folders() {
    let environment = new_environment();
    let folder = PathBuf::from("/project");
    let other_folder = PathBuf::from("/other");
    assert!(!is_no_config_notification_dismissed(&environment, std::slice::from_ref(&folder)));

    store_dismissal(&environment, NoConfigNotificationDismissal::WorkspaceFolders(vec![folder.clone()])).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, std::slice::from_ref(&folder)));
    assert!(is_no_config_notification_dismissed(&environment, &[other_folder.clone(), folder.clone()]));
    assert!(!is_no_config_notification_dismissed(&environment, std::slice::from_ref(&other_folder)));
    assert!(!is_no_config_notification_dismissed(&environment, &[]));

    // keeps the existing folders
    store_dismissal(&environment, NoConfigNotificationDismissal::WorkspaceFolders(vec![other_folder.clone()])).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[folder]));
    assert!(is_no_config_notification_dismissed(&environment, &[other_folder]));
  }

  #[test]
  fn dismisses_everywhere() {
    let environment = new_environment();
    store_dismissal(&environment, NoConfigNotificationDismissal::Everywhere).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[]));
    assert!(is_no_config_notification_dismissed(&environment, &[PathBuf::from("/project")]));
  }

  #[test]
  fn ignores_invalid_state_file() {
    let environment = new_environment();
    environment.mk_dir_all("/global-config").unwrap();
    environment.write_file(get_state_file_path(&environment).unwrap(), "not json").unwrap();
    assert!(!is_no_config_notification_dismissed(&environment, &[]));
    store_dismissal(&environment, NoConfigNotificationDismissal::Everywhere).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[]));
  }

  fn new_environment() -> TestEnvironment {
    let environment = TestEnvironment::new();
    environment.set_env_var("DPRINT_CONFIG_DIR", Some("/global-config"));
    environment
  }
}
