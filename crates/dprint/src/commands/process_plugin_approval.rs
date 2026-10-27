use anyhow::Result;
use parking_lot::Mutex;

use crate::configuration::ProcessPluginApproval;
use crate::configuration::ProcessPluginDecision;
use crate::configuration::store_process_plugin_decision;
use crate::environment::Environment;

/// The actions of the message that asks the user whether to allow running the
/// process plugins of a config file, in the order they're shown.
pub const PROCESS_PLUGIN_ACTION_TITLES: [&str; 4] = [ALLOW_ACTION_TITLE, ALWAYS_ALLOW_ACTION_TITLE, DENY_ACTION_TITLE, ALWAYS_DENY_ACTION_TITLE];

/// Stores what the user decided about running the process plugins from the
/// action they chose, which is only for the session unless they chose to always
/// allow or deny them. Not choosing an action denies them for the session.
pub async fn store_process_plugin_action<TEnvironment: Environment>(
  environment: &TEnvironment,
  session_decisions: &Mutex<Vec<ProcessPluginDecision>>,
  approval: ProcessPluginApproval,
  action_title: Option<&str>,
) -> Result<()> {
  let decision = ProcessPluginDecision {
    approval,
    allowed: matches!(action_title, Some(ALLOW_ACTION_TITLE | ALWAYS_ALLOW_ACTION_TITLE)),
  };
  if matches!(action_title, Some(ALWAYS_ALLOW_ACTION_TITLE | ALWAYS_DENY_ACTION_TITLE)) {
    store_process_plugin_decision(environment, decision).await
  } else {
    session_decisions.lock().push(decision);
    Ok(())
  }
}

const ALLOW_ACTION_TITLE: &str = "Allow";
const ALWAYS_ALLOW_ACTION_TITLE: &str = "Always Allow";
const DENY_ACTION_TITLE: &str = "Deny";
const ALWAYS_DENY_ACTION_TITLE: &str = "Always Deny";
