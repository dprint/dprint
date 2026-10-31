use anyhow::Result;

use crate::commands::lsp::workspace::messages::ApprovalDecision;
use crate::configuration::NewerApprovalsFileError;
use crate::configuration::ProcessPluginApproval;
use crate::configuration::ProcessPluginDecision;
use crate::configuration::ProcessPluginSetUpGrant;
use crate::configuration::ResolvedConfig;
use crate::configuration::SessionPermissions;
use crate::configuration::read_editor_approvals;
use crate::configuration::read_ignored_settings_message;
use crate::configuration::store_process_plugin_decision;
use crate::configuration::update_editor_approvals;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::resolution::UnapprovedProcessPlugins;
use crate::resolution::get_process_plugin_approvals;
use crate::resolution::get_trusted_config_hash;
use crate::resolution::get_unpinned_plugin_file_checksum;

/// The actions of the message that asks the user whether to allow running the
/// process plugins of a config file, in the order they're shown.
pub const PROCESS_PLUGIN_ACTION_TITLES: [&str; 4] = [ALLOW_ACTION_TITLE, ALWAYS_ALLOW_ACTION_TITLE, DENY_ACTION_TITLE, ALWAYS_DENY_ACTION_TITLE];

/// Stores what the user decided about running the process plugins from the
/// action they chose, which is only for the session unless they chose to always
/// allow or deny them. Not choosing an action (ex. closing the message) leaves
/// it undecided, so they can be asked again.
///
/// A plugin that isn't set up is only allowed to be set up with the trusted
/// configuration of the config file because its configuration isn't known
/// until then. It's allowed with its configuration once it's set up (see
/// `record_set_up_process_plugins`). Denying a plugin denies it with any
/// configuration so that changing it doesn't ask the user again.
///
/// The decision is used for the session even when storing it fails, and a
/// plugin it allows that isn't set up is then only allowed for the session
/// too, so that the decision isn't partly stored later.
pub async fn store_process_plugin_action<TEnvironment: Environment>(
  environment: &TEnvironment,
  session_permissions: &SessionPermissions,
  unapproved: &UnapprovedProcessPlugins,
  action_title: Option<&str>,
) -> Result<()> {
  let allowed = match action_title {
    Some(ALLOW_ACTION_TITLE | ALWAYS_ALLOW_ACTION_TITLE) => true,
    Some(DENY_ACTION_TITLE | ALWAYS_DENY_ACTION_TITLE) => false,
    // the editor responded with an action that wasn't offered
    Some(_) | None => return Ok(()),
  };
  let always = matches!(action_title, Some(ALWAYS_ALLOW_ACTION_TITLE | ALWAYS_DENY_ACTION_TITLE));
  // what the user decided now is more recent than what they decided before
  let decisions = session_permissions.read(environment).process_plugin_decisions;
  let mut result = Ok(());
  let mut not_set_up = Vec::new();
  for plugin in &unapproved.plugins {
    let approval = match &plugin.approval {
      _ if !allowed => {
        // it's not set up once it's denied
        session_permissions.take_process_plugin_set_up_grant(&unapproved.config_file, &plugin.plugin);
        ProcessPluginApproval::any_config(unapproved.config_file.clone(), plugin.plugin.clone())
      }
      Some(approval) => approval.clone(),
      None => {
        not_set_up.push(plugin);
        continue;
      }
    };
    let decision = ProcessPluginDecision::new(environment, approval, allowed, &decisions);
    if let Err(err) = add_process_plugin_decision(environment, session_permissions, decision, always).await {
      result = Err(err);
    }
  }
  // the decision is only used for the session once storing it failed
  let always = always && result.is_ok();
  for plugin in &not_set_up {
    session_permissions.add_process_plugin_set_up_grant(ProcessPluginSetUpGrant::new(
      environment,
      unapproved.config_file.clone(),
      plugin.plugin.clone(),
      unapproved.trusted_config_hash.clone(),
      plugin.unpinned_file_checksum.clone(),
      always,
      &decisions,
    ));
  }
  result
}

/// Allows the process plugins that the user allowed setting up with their
/// configuration now that they're set up and it's known, which is remembered
/// when the user chose to always allow them.
pub async fn record_set_up_process_plugins<TEnvironment: Environment>(
  environment: &TEnvironment,
  session_permissions: &SessionPermissions,
  config: &ResolvedConfig,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<()> {
  let mut result = Ok(());
  let trusted_config_hash = get_trusted_config_hash(config);
  let permissions = session_permissions.read(environment);
  for (plugin, approval) in get_process_plugin_approvals(config, plugin_resolver, environment) {
    let Some(approval) = approval else {
      continue;
    };
    let Some(grant) = session_permissions.take_process_plugin_set_up_grant(&approval.config_file, &approval.plugin) else {
      continue;
    };
    // IMPORTANT: The plugin may have been set up elsewhere (ex. by another
    // config file), so only allow it with the configuration the user saw,
    // and with the file of an npm plugin without a version they saw (ex. it
    // wasn't updated since).
    if grant.trusted_config_hash != trusted_config_hash || grant.unpinned_file_checksum != get_unpinned_plugin_file_checksum(&plugin, environment) {
      continue;
    }
    // IMPORTANT: Don't replace what the user denied since (ex. "Always Deny" in another window).
    if permissions.is_denied_since(&grant) {
      continue;
    }
    // it's when the user allowed it, so a denial since then (ex. one stored
    // after the permissions were read) is more recent and wins
    let decision = ProcessPluginDecision {
      approval,
      allowed: true,
      decided_at: grant.decided_at,
    };
    if let Err(err) = add_process_plugin_decision(environment, session_permissions, decision, grant.always).await {
      result = Err(err);
    }
  }
  result
}

/// Allows the editors to run the process plugins of the config file with its
/// configuration like `allow_process_plugins_in_editors`, which only warns
/// when that fails because the user is using the CLI.
pub async fn allow_process_plugins_in_editors_or_warn<TEnvironment: Environment>(
  environment: &TEnvironment,
  config: &ResolvedConfig,
  plugin_resolver: &PluginResolver<TEnvironment>,
) {
  if let Err(err) = allow_process_plugins_in_editors(environment, config, plugin_resolver).await {
    log_warn!(
      environment,
      "Failed allowing the process plugins of {} in editors: {:#}",
      config.source.display(),
      err
    );
  }
}

/// Allows the editors to run the process plugins of the config file with its
/// configuration, which is for when the user ran dprint with it (`dprint fmt`,
/// `dprint check`, or `dprint config install`) because that's the same as choosing to run
/// them. The plugins need to be set up.
///
/// Nothing is stored when the approvals file can't be read (ex. it's from a
/// newer version of dprint that this version can't use, which this version
/// never writes to), which is only logged in debug mode because it would be
/// on each run.
pub async fn allow_process_plugins_in_editors<TEnvironment: Environment>(
  environment: &TEnvironment,
  config: &ResolvedConfig,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<()> {
  let approvals = get_process_plugin_approvals(config, plugin_resolver, environment)
    .into_iter()
    .filter_map(|(_, approval)| approval)
    .collect::<Vec<_>>();
  if approvals.is_empty() {
    return Ok(());
  }
  let stored = match read_editor_approvals(environment) {
    Ok(approvals) => approvals,
    Err(err) => {
      log_debug!(
        environment,
        "Not allowing the process plugins of {} in editors. {:#}",
        config.source.display(),
        err
      );
      return Ok(());
    }
  };
  let decisions = approvals
    .into_iter()
    // the approvals file is only written when something changed, which is usually not, and an allow
    // that's stored by a version of dprint that makes its config hash another way doesn't change it
    .filter(|approval| !stored.is_process_plugin_allowed(approval))
    // running dprint is more recent than what the user decided before
    .map(|approval| ProcessPluginDecision::new(environment, approval, true, &stored.process_plugin_decisions))
    .collect::<Vec<_>>();
  if decisions.is_empty() {
    return Ok(());
  }
  update_editor_approvals(environment, |approvals| {
    for decision in decisions {
      approvals.add_process_plugin_decision(decision);
    }
  })
  .await
}

/// Gets the message for the user about a failure to store a decision for
/// process plugins that they chose to always use, which is then only used for
/// the session. The approvals file being from a newer version of dprint that
/// this version can't write to isn't a failure of its own, so that message
/// says to update dprint instead (see `EditorApprovals`).
pub fn get_process_plugin_store_error_message(err: &anyhow::Error) -> String {
  match err.chain().find_map(|err| err.downcast_ref::<NewerApprovalsFileError>()) {
    Some(err) => format!(
      "This version of dprint is older than the one that stored the approval decisions in {}, so this decision is only kept until the editor restarts. Update dprint to remember it.",
      err.file_path.display()
    ),
    None => format!("Failed storing the decision for the process plugins: {:#}", err),
  }
}

/// Gets the title of the action of a decision, which is what
/// `store_process_plugin_action` stores.
pub fn get_process_plugin_action_title(decision: ApprovalDecision) -> &'static str {
  match decision {
    ApprovalDecision::Allow => ALLOW_ACTION_TITLE,
    ApprovalDecision::AlwaysAllow => ALWAYS_ALLOW_ACTION_TITLE,
    ApprovalDecision::Deny => DENY_ACTION_TITLE,
    ApprovalDecision::AlwaysDeny => ALWAYS_DENY_ACTION_TITLE,
  }
}

/// Gets the decision of the action the user chose, which is `None` for an
/// action that wasn't offered.
pub fn get_process_plugin_decision(action_title: &str) -> Option<ApprovalDecision> {
  match action_title {
    ALLOW_ACTION_TITLE => Some(ApprovalDecision::Allow),
    ALWAYS_ALLOW_ACTION_TITLE => Some(ApprovalDecision::AlwaysAllow),
    DENY_ACTION_TITLE => Some(ApprovalDecision::Deny),
    ALWAYS_DENY_ACTION_TITLE => Some(ApprovalDecision::AlwaysDeny),
    _ => None,
  }
}

const ALLOW_ACTION_TITLE: &str = "Allow";
const ALWAYS_ALLOW_ACTION_TITLE: &str = "Always Allow";
const DENY_ACTION_TITLE: &str = "Deny";
const ALWAYS_DENY_ACTION_TITLE: &str = "Always Deny";

/// Uses the decision, which is stored when it's always used. A decision that's
/// stored is only read from the approvals file so that changing the file (ex.
/// deleting it) is used, and the session uses it when storing it fails. The
/// session also uses an allow that's stored while the editors don't use the
/// settings, because they don't use the stored allows then (see `Settings`).
async fn add_process_plugin_decision<TEnvironment: Environment>(
  environment: &TEnvironment,
  session_permissions: &SessionPermissions,
  decision: ProcessPluginDecision,
  always: bool,
) -> Result<()> {
  if always {
    let _store = session_permissions.start_store().await;
    let result = store_process_plugin_decision(environment, decision.clone()).await;
    if result.is_err() || (decision.allowed && read_ignored_settings_message(environment).is_some()) {
      session_permissions.add_process_plugin_decision(decision);
    }
    result
  } else {
    session_permissions.add_process_plugin_decision(decision);
    Ok(())
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::configuration::get_editor_approvals_file_path;
  use crate::configuration::get_settings_file_path;
  use crate::environment::TestEnvironment;
  use crate::resolution::UnapprovedProcessPlugin;

  const PLUGIN: &str = "https://plugins.dprint.dev/plugin.json@checksum";

  fn approval() -> ProcessPluginApproval {
    ProcessPluginApproval {
      config_file: "/dprint.json".to_string(),
      plugin: PLUGIN.to_string(),
      config_hash: Some("1:a".to_string()),
      config_key: None,
    }
  }

  fn unapproved(approval: Option<ProcessPluginApproval>) -> UnapprovedProcessPlugins {
    UnapprovedProcessPlugins {
      config_file: "/dprint.json".to_string(),
      trusted_config_hash: "hash".to_string(),
      plugins: vec![UnapprovedProcessPlugin {
        plugin: PLUGIN.to_string(),
        registry: None,
        unpinned_file_checksum: None,
        approval,
      }],
    }
  }

  fn require_approval(environment: &TestEnvironment) {
    let settings_file_path = get_settings_file_path(environment).unwrap();
    environment.mk_dir_all(settings_file_path.parent().unwrap()).unwrap();
    environment
      .write_file(&settings_file_path, r#"{ "editor": { "allowProcessPlugins": false } }"#)
      .unwrap();
  }

  #[test]
  fn should_leave_undecided_without_an_action() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      let unapproved = unapproved(Some(approval()));
      store_process_plugin_action(&environment, &session_permissions, &unapproved, None)
        .await
        .unwrap();
      assert!(!session_permissions.read(&environment).is_process_plugin_denied(&approval()));
      store_process_plugin_action(&environment, &session_permissions, &unapproved, Some(DENY_ACTION_TITLE))
        .await
        .unwrap();
      assert!(session_permissions.read(&environment).is_process_plugin_denied(&approval()));
    });
  }

  #[test]
  fn should_only_allow_setting_up_a_plugin_that_is_not_set_up() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      store_process_plugin_action(&environment, &session_permissions, &unapproved(None), Some(ALWAYS_ALLOW_ACTION_TITLE))
        .await
        .unwrap();
      let permissions = session_permissions.read(&environment);
      assert!(permissions.may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", None));
      // its configuration isn't known yet, so nothing is stored
      assert!(!permissions.is_process_plugin_allowed(&approval()));
      assert!(read_editor_approvals(&environment).unwrap().process_plugin_decisions.is_empty());
    });
  }

  #[test]
  fn should_use_an_always_allow_for_the_session_when_the_settings_are_not_used() {
    let environment = TestEnvironment::new();
    let settings_file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(settings_file_path.parent().unwrap()).unwrap();
    // ex. a setting of a newer version of dprint, which has the stored allows not be used
    environment
      .write_file(&settings_file_path, r#"{ "editor": { "useStoredApprovals": false } }"#)
      .unwrap();
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      store_process_plugin_action(
        &environment,
        &session_permissions,
        &unapproved(Some(approval())),
        Some(ALWAYS_ALLOW_ACTION_TITLE),
      )
      .await
      .unwrap();
      // it's stored for the versions that use the settings
      assert_eq!(read_editor_approvals(&environment).unwrap().process_plugin_decisions.len(), 1);
      assert!(session_permissions.read(&environment).is_process_plugin_allowed(&approval()));
      // but isn't used by another session
      assert!(!SessionPermissions::default().read(&environment).is_process_plugin_allowed(&approval()));

      // the same goes for a plugin that's allowed once it's set up
      let session_permissions = SessionPermissions::default();
      let grant = ProcessPluginSetUpGrant::new(
        &environment,
        "/dprint.json".to_string(),
        PLUGIN.to_string(),
        "hash".to_string(),
        None,
        true,
        &[],
      );
      let decision = ProcessPluginDecision {
        approval: approval(),
        allowed: true,
        decided_at: grant.decided_at,
      };
      add_process_plugin_decision(&environment, &session_permissions, decision, grant.always)
        .await
        .unwrap();
      assert!(session_permissions.read(&environment).is_process_plugin_allowed(&approval()));
      // and a denial is only stored, because the stored denials are used
      let session_permissions = SessionPermissions::default();
      store_process_plugin_action(
        &environment,
        &session_permissions,
        &unapproved(Some(approval())),
        Some(ALWAYS_DENY_ACTION_TITLE),
      )
      .await
      .unwrap();
      assert!(session_permissions.session_process_plugin_decisions().is_empty());
      assert!(SessionPermissions::default().read(&environment).is_process_plugin_denied(&approval()));
      environment.take_stderr_messages();
    });
  }

  #[test]
  fn should_only_allow_setting_up_the_file_of_an_unpinned_npm_plugin_that_was_allowed() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      let mut not_set_up = unapproved(None);
      not_set_up.plugins[0].unpinned_file_checksum = Some("file".to_string());
      store_process_plugin_action(&environment, &session_permissions, &not_set_up, Some(ALLOW_ACTION_TITLE))
        .await
        .unwrap();
      let permissions = session_permissions.read(&environment);
      assert!(permissions.may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", Some("file")));
      // ex. the package was updated since
      assert!(!permissions.may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", Some("other")));
      assert!(!permissions.may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", None));
    });
  }

  #[test]
  fn should_deny_a_plugin_that_is_not_set_up_with_any_config() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      let not_set_up = unapproved(None);
      store_process_plugin_action(&environment, &session_permissions, &not_set_up, Some(DENY_ACTION_TITLE))
        .await
        .unwrap();
      assert!(not_set_up.is_denied(&session_permissions.read(&environment)));
      // denying it for the session isn't stored
      assert!(read_editor_approvals(&environment).unwrap().process_plugin_decisions.is_empty());
      assert!(!not_set_up.is_denied(&SessionPermissions::default().read(&environment)));

      store_process_plugin_action(&environment, &session_permissions, &not_set_up, Some(ALWAYS_DENY_ACTION_TITLE))
        .await
        .unwrap();
      let permissions = SessionPermissions::default().read(&environment);
      assert!(not_set_up.is_denied(&permissions));
      // it stays denied once it's set up because what was denied is the plugin
      assert!(unapproved(Some(approval())).is_denied(&permissions));
      assert!(!permissions.may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", None));
    });
  }

  #[test]
  fn should_deny_a_plugin_that_is_set_up_with_any_config() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      store_process_plugin_action(
        &environment,
        &session_permissions,
        &unapproved(Some(approval())),
        Some(ALWAYS_DENY_ACTION_TITLE),
      )
      .await
      .unwrap();
      // changing its configuration doesn't ask the user again
      let other_config = ProcessPluginApproval {
        config_hash: Some("1:b".to_string()),
        config_key: None,
        ..approval()
      };
      let permissions = SessionPermissions::default().read(&environment);
      assert!(permissions.is_process_plugin_denied(&approval()));
      assert!(permissions.is_process_plugin_denied(&other_config));
    });
  }

  #[test]
  fn should_use_what_the_user_decided_after_a_decision_from_the_future() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    environment.set_fs_time(5);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      // ex. from a machine whose clock is ahead, or the clock was set back since
      let denied = ProcessPluginDecision {
        approval: ProcessPluginApproval::any_config("/dprint.json".to_string(), PLUGIN.to_string()),
        allowed: false,
        decided_at: 10_000,
      };
      store_process_plugin_decision(&environment, denied).await.unwrap();
      store_process_plugin_action(&environment, &session_permissions, &unapproved(None), Some(ALWAYS_ALLOW_ACTION_TITLE))
        .await
        .unwrap();
      assert!(
        session_permissions
          .read(&environment)
          .may_set_up_process_plugin("/dprint.json", PLUGIN, "hash", None)
      );
      store_process_plugin_action(
        &environment,
        &session_permissions,
        &unapproved(Some(approval())),
        Some(ALWAYS_ALLOW_ACTION_TITLE),
      )
      .await
      .unwrap();
      let decisions = read_editor_approvals(&environment).unwrap().process_plugin_decisions;
      assert_eq!(
        decisions,
        vec![ProcessPluginDecision {
          approval: approval(),
          allowed: true,
          decided_at: 10_001,
        }]
      );
      assert!(SessionPermissions::default().read(&environment).is_process_plugin_allowed(&approval()));
    });
  }

  #[test]
  fn should_use_decision_for_session_when_storing_it_fails() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    // writing the approvals file fails when it's a directory
    environment.mk_dir_all(get_editor_approvals_file_path(&environment).unwrap()).unwrap();
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      assert!(
        store_process_plugin_action(
          &environment,
          &session_permissions,
          &unapproved(Some(approval())),
          Some(ALWAYS_ALLOW_ACTION_TITLE)
        )
        .await
        .is_err()
      );
      assert!(session_permissions.read(&environment).is_process_plugin_allowed(&approval()));
      // reading what's stored failed too, which nothing is allowed by
      let messages = environment.take_stderr_messages();
      assert_eq!(messages.len(), 1);
      assert!(messages[0].starts_with("Asking to allow process plugins again in editors."), "{}", messages[0]);
    });
  }

  #[test]
  fn should_use_decision_for_session_when_the_lock_of_the_approvals_file_fails() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let approvals_file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. the file system doesn't support locks, so it's never written without one
    environment.set_file_lock_fails(approvals_file_path.with_file_name(".editor-approvals.lock"), true);
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      let err = store_process_plugin_action(
        &environment,
        &session_permissions,
        &unapproved(Some(approval())),
        Some(ALWAYS_ALLOW_ACTION_TITLE),
      )
      .await
      .unwrap_err();
      assert!(get_process_plugin_store_error_message(&err).contains("Failed locking"), "{:#}", err);
      assert!(!environment.path_exists(&approvals_file_path));
      assert!(session_permissions.read(&environment).is_process_plugin_allowed(&approval()));
    });
  }

  #[test]
  fn should_only_allow_setting_up_a_plugin_for_the_session_when_storing_the_decision_fails() {
    const OTHER_PLUGIN: &str = "https://plugins.dprint.dev/other.json@checksum";
    let environment = TestEnvironment::new();
    require_approval(&environment);
    // writing the approvals file fails when it's a directory
    environment.mk_dir_all(get_editor_approvals_file_path(&environment).unwrap()).unwrap();
    let session_permissions = SessionPermissions::default();

    environment.clone().run_in_runtime(async move {
      let mut unapproved = unapproved(Some(approval()));
      unapproved.plugins.push(UnapprovedProcessPlugin {
        plugin: OTHER_PLUGIN.to_string(),
        registry: None,
        unpinned_file_checksum: None,
        approval: None,
      });
      assert!(
        store_process_plugin_action(&environment, &session_permissions, &unapproved, Some(ALWAYS_ALLOW_ACTION_TITLE))
          .await
          .is_err()
      );
      // so the decision isn't partly stored once it's set up
      let grant = session_permissions.take_process_plugin_set_up_grant("/dprint.json", OTHER_PLUGIN).unwrap();
      assert!(!grant.always);
      environment.take_stderr_messages();
    });
  }
}
