use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;

use crate::configuration::EditorState;
use crate::configuration::read_editor_state;
use crate::configuration::update_editor_state;
use crate::environment::Environment;

pub const DISMISS_ACTION_TITLE: &str = "Don't show again";
pub const DISMISS_WORKSPACE_ACTION_TITLE: &str = "Don't show in this workspace";
pub const LANGUAGE_SERVER_SETTINGS_OWNER: &str = "the dprint language server";
pub const EDITOR_EXTENSION_SETTINGS_OWNER: &str = "the dprint extension";

pub struct NoConfigMessageOptions {
  /// Whether the global config file would have been used.
  pub use_global_config: bool,
  /// Whether there's a global config file that isn't being used.
  pub has_global_config: bool,
  /// What has the `useGlobalConfig` setting (ex. "the dprint language server").
  pub settings_owner: &'static str,
}

/// Gets the message to show when no config file was found for a file.
pub fn get_no_config_message(options: NoConfigMessageOptions) -> String {
  let message = "No dprint configuration file found. Run \"dprint init\" in your project to create one";
  if options.use_global_config {
    format!("{} or \"dprint init --global\" to create a global one.", message)
  } else if options.has_global_config {
    format!(
      "{} or enable the \"useGlobalConfig\" setting of {} to use your global one.",
      message, options.settings_owner
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
  let state = read_editor_state(environment).no_config_notification;
  state.dismissed
    || workspace_folders
      .iter()
      .any(|folder| state.dismissed_workspace_folders.contains(&folder_key(folder)))
}

/// Stores that the user dismissed the notification. A server can't update the
/// editor's settings, so this is stored in the cache directory instead.
pub async fn dismiss_no_config_notification<TEnvironment: Environment>(environment: &TEnvironment, dismissal: NoConfigNotificationDismissal) -> Result<()> {
  update_editor_state(environment, |state| add_dismissal(state, dismissal)).await
}

fn add_dismissal(state: &mut EditorState, dismissal: NoConfigNotificationDismissal) {
  match dismissal {
    NoConfigNotificationDismissal::Everywhere => state.no_config_notification.dismissed = true,
    NoConfigNotificationDismissal::WorkspaceFolders(folders) => {
      state
        .no_config_notification
        .dismissed_workspace_folders
        .extend(folders.iter().map(|folder| folder_key(folder)));
    }
  }
}

/// Folders are compared case insensitively because clients differ in the
/// casing of the paths they provide (ex. of the drive letter on Windows).
fn folder_key(folder: &Path) -> String {
  folder.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::configuration::get_editor_state_file_path;
  use crate::configuration::update_editor_state_without_lock;
  use crate::environment::TestEnvironment;

  fn store_dismissal(environment: &TestEnvironment, dismissal: NoConfigNotificationDismissal) -> Result<()> {
    update_editor_state_without_lock(environment, |state| add_dismissal(state, dismissal))
  }

  #[test]
  fn no_config_message() {
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: true,
        has_global_config: false,
        settings_owner: LANGUAGE_SERVER_SETTINGS_OWNER,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one or \"dprint init --global\" to create a global one."
    );
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: false,
        has_global_config: true,
        settings_owner: LANGUAGE_SERVER_SETTINGS_OWNER,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one or enable the \"useGlobalConfig\" setting of the dprint language server to use your global one."
    );
    assert_eq!(
      get_no_config_message(NoConfigMessageOptions {
        use_global_config: false,
        has_global_config: false,
        settings_owner: LANGUAGE_SERVER_SETTINGS_OWNER,
      }),
      "No dprint configuration file found. Run \"dprint init\" in your project to create one."
    );
  }

  #[test]
  fn dismisses_for_workspace_folders() {
    let environment = TestEnvironment::new();
    let folder = PathBuf::from("/project");
    let other_folder = PathBuf::from("/other");
    assert!(!is_no_config_notification_dismissed(&environment, std::slice::from_ref(&folder)));

    store_dismissal(&environment, NoConfigNotificationDismissal::WorkspaceFolders(vec![folder.clone()])).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, std::slice::from_ref(&folder)));
    assert!(is_no_config_notification_dismissed(&environment, &[other_folder.clone(), folder.clone()]));
    assert!(!is_no_config_notification_dismissed(&environment, std::slice::from_ref(&other_folder)));
    assert!(!is_no_config_notification_dismissed(&environment, &[]));
    // case insensitive
    assert!(is_no_config_notification_dismissed(&environment, &[PathBuf::from("/PROJECT")]));

    // keeps the existing folders
    store_dismissal(&environment, NoConfigNotificationDismissal::WorkspaceFolders(vec![other_folder.clone()])).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[folder]));
    assert!(is_no_config_notification_dismissed(&environment, &[other_folder]));
  }

  #[test]
  fn dismisses_everywhere() {
    let environment = TestEnvironment::new();
    store_dismissal(&environment, NoConfigNotificationDismissal::Everywhere).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[]));
    assert!(is_no_config_notification_dismissed(&environment, &[PathBuf::from("/project")]));
  }

  #[test]
  fn ignores_invalid_state_file() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all(environment.get_cache_dir()).unwrap();
    environment.write_file(get_editor_state_file_path(&environment), "not json").unwrap();
    assert!(!is_no_config_notification_dismissed(&environment, &[]));
    store_dismissal(&environment, NoConfigNotificationDismissal::Everywhere).unwrap();
    assert!(is_no_config_notification_dismissed(&environment, &[]));
  }
}
