use anyhow::Result;

use crate::configuration::ProcessPluginApproval;
use crate::configuration::ProcessPluginDecision;
use crate::configuration::SessionPermissions;
use crate::configuration::store_process_plugin_decision;
use crate::environment::Environment;

/// The actions of the message that asks the user whether to allow running the
/// process plugins of a config file, in the order they're shown.
pub const PROCESS_PLUGIN_ACTION_TITLES: [&str; 4] = [ALLOW_ACTION_TITLE, ALWAYS_ALLOW_ACTION_TITLE, DENY_ACTION_TITLE, ALWAYS_DENY_ACTION_TITLE];

/// Stores what the user decided about running the process plugins from the
/// action they chose, which is only for the session unless they chose to always
/// allow or deny them. Not choosing an action denies them for the session.
///
/// The decision is used for the session even when storing it fails.
pub async fn store_process_plugin_action<TEnvironment: Environment>(
  environment: &TEnvironment,
  session_permissions: &SessionPermissions,
  approval: ProcessPluginApproval,
  action_title: Option<&str>,
) -> Result<()> {
  let allowed = matches!(action_title, Some(ALLOW_ACTION_TITLE | ALWAYS_ALLOW_ACTION_TITLE));
  let decision = ProcessPluginDecision::new(environment, approval, allowed);
  session_permissions.add_process_plugin_decision(decision.clone());
  if matches!(action_title, Some(ALWAYS_ALLOW_ACTION_TITLE | ALWAYS_DENY_ACTION_TITLE)) {
    store_process_plugin_decision(environment, decision).await
  } else {
    Ok(())
  }
}

const ALLOW_ACTION_TITLE: &str = "Allow";
const ALWAYS_ALLOW_ACTION_TITLE: &str = "Always Allow";
const DENY_ACTION_TITLE: &str = "Deny";
const ALWAYS_DENY_ACTION_TITLE: &str = "Always Deny";

#[cfg(test)]
mod test {
  use super::*;
  use crate::configuration::get_editor_state_file_path;
  use crate::configuration::get_settings_file_path;
  use crate::environment::TestEnvironment;

  #[test]
  fn should_use_decision_for_session_when_storing_it_fails() {
    let environment = TestEnvironment::new();
    let settings_file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(settings_file_path.parent().unwrap()).unwrap();
    environment
      .write_file(&settings_file_path, r#"{ "editor": { "allowProcessPlugins": false } }"#)
      .unwrap();
    // writing the state file fails when it's a directory
    environment.mk_dir_all(get_editor_state_file_path(&environment)).unwrap();
    let approval = ProcessPluginApproval {
      config_file: "/dprint.json".to_string(),
      config_hash: "1".to_string(),
    };
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      assert!(
        store_process_plugin_action(&environment, &session_permissions, approval.clone(), Some(ALWAYS_ALLOW_ACTION_TITLE))
          .await
          .is_err()
      );
      assert!(session_permissions.read(&environment).is_process_plugin_allowed(&approval));
    });
  }
}
