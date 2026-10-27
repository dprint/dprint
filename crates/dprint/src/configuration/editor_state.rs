use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;
use crate::utils::LaxSingleProcessFsFlag;

use super::get_settings_file_path;
use super::read_settings;

/// Name of the file in the cache directory with what the editor integrations
/// (the language server and the editor service) remember, such as the user's
/// choices.
const STATE_FILE_NAME: &str = "editor-state.json";
/// Name of the file the language server used before (dprint 0.60 to 0.64).
const LEGACY_STATE_FILE_NAME: &str = "lsp-state.json";
/// The most process plugin decisions to remember, where the oldest are
/// forgotten first, which only means the user is asked again.
const MAX_PROCESS_PLUGIN_DECISIONS: usize = 5000;

/// What the editor integrations remember.
///
/// It's in the cache directory because losing it only has the user asked
/// again. Clearing the cache also discards the list of workspace folders that
/// builds up in it.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorState {
  pub no_config_notification: NoConfigNotificationState,
  /// The process plugin decisions the user said to always use, with the
  /// most recent last.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoConfigNotificationState {
  pub dismissed: bool,
  /// The lowercased paths of the folders, which are in a sorted set so that
  /// finding one doesn't go over every folder.
  pub dismissed_workspace_folders: BTreeSet<String>,
}

/// The process plugins of a config file with its configuration, which is what
/// the user allows or denies running.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginApproval {
  /// The config file that specifies the process plugins.
  pub config_file: String,
  /// The SHA-256 checksum of the config file's resolved plugins and
  /// configuration, so a change to it (ex. the commands of
  /// dprint-plugin-exec) needs to be decided on again.
  pub config_hash: String,
}

/// Whether the user allowed or denied running the process plugins of a config
/// file with its configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginDecision {
  #[serde(flatten)]
  pub approval: ProcessPluginApproval,
  pub allowed: bool,
  /// When the user decided, in milliseconds since the Unix epoch, so that the
  /// most recent decision wins (ex. allowing them with `dprint config
  /// editor-approve` after denying them for the session).
  #[serde(default)]
  pub decided_at: u64,
}

impl ProcessPluginDecision {
  /// Creates the decision the user made now.
  pub fn new(environment: &impl Environment, approval: ProcessPluginApproval, allowed: bool) -> Self {
    let decided_at = environment
      .sys_time_now()
      .duration_since(UNIX_EPOCH)
      .map(|duration| duration.as_millis() as u64)
      .unwrap_or(0);
    Self { approval, allowed, decided_at }
  }
}

/// What the user allowed the editor integrations to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Permissions {
  /// Whether process plugins run without the user approving them.
  pub allow_process_plugins: Option<bool>,
  /// The decisions to always use followed by the ones for the session, where
  /// the most recent one for a config file wins.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
}

impl Permissions {
  pub fn is_process_plugin_allowed(&self, approval: &ProcessPluginApproval) -> bool {
    // todo(dsherret): default to asking in a future version
    self.allow_process_plugins.unwrap_or(true) || self.get_decision(approval) == Some(true)
  }

  /// Gets whether the user denied running the process plugins, which means
  /// they shouldn't be asked about them.
  pub fn is_process_plugin_denied(&self, approval: &ProcessPluginApproval) -> bool {
    self.get_decision(approval) == Some(false)
  }

  /// Gets whether the process plugins of the config file were allowed when it
  /// had a different configuration.
  pub fn was_allowed_with_other_config(&self, approval: &ProcessPluginApproval) -> bool {
    self
      .process_plugin_decisions
      .iter()
      .any(|decision| decision.allowed && decision.approval.config_file == approval.config_file && decision.approval.config_hash != approval.config_hash)
  }

  fn get_decision(&self, approval: &ProcessPluginApproval) -> Option<bool> {
    self
      .process_plugin_decisions
      .iter()
      .filter(|decision| decision.approval == *approval)
      // the last one wins a tie, which is the one for the session
      .max_by_key(|decision| decision.decided_at)
      .map(|decision| decision.allowed)
  }
}

/// What an editor integration keeps for its session about what the user
/// allowed it to do, which is shared by its threads.
#[derive(Debug, Default)]
pub struct SessionPermissions {
  /// What the user decided about running process plugins for the session.
  process_plugin_decisions: Mutex<Vec<ProcessPluginDecision>>,
  /// The problems with the settings that were logged, which are only logged
  /// once because the settings are read for each request.
  logged_settings_problems: Mutex<HashSet<String>>,
}

impl SessionPermissions {
  /// Reads what the user allowed the editor integrations to do, along with
  /// what they decided for the session.
  pub fn read(&self, environment: &impl Environment) -> Permissions {
    let allow_process_plugins = match read_settings(environment) {
      Ok(Some(settings)) => {
        for property_name in &settings.unknown_properties {
          let file_path = get_settings_file_path(environment).map(|path| path.display().to_string()).unwrap_or_default();
          self.log_settings_problem(environment, format!("Unknown property in {}: {}", file_path, property_name));
        }
        settings.editor.allow_process_plugins
      }
      Ok(None) => None,
      Err(err) => {
        self.log_settings_problem(
          environment,
          format!(
            "Requiring approval to run process plugins in editors because the settings couldn't be read: {:#}",
            err
          ),
        );
        // IMPORTANT: Don't allow what the user may have tried to disallow.
        Some(false)
      }
    };
    // only read the decisions when they're used
    let mut process_plugin_decisions = if allow_process_plugins == Some(false) {
      read_editor_state(environment).process_plugin_decisions
    } else {
      Vec::new()
    };
    process_plugin_decisions.extend(self.process_plugin_decisions.lock().iter().cloned());
    Permissions {
      allow_process_plugins,
      process_plugin_decisions,
    }
  }

  /// Uses the decision for the rest of the session.
  pub fn add_process_plugin_decision(&self, decision: ProcessPluginDecision) {
    self.process_plugin_decisions.lock().push(decision);
  }

  fn log_settings_problem(&self, environment: &impl Environment, message: String) {
    if self.logged_settings_problems.lock().insert(message.clone()) {
      log_warn!(environment, "{}", message);
    }
  }
}

/// Remembers the user's decision for running the process plugins of a config
/// file, which replaces the one for a previous configuration of it.
pub async fn store_process_plugin_decision(environment: &impl Environment, decision: ProcessPluginDecision) -> Result<()> {
  update_editor_state(environment, |state| add_process_plugin_decision(state, decision)).await
}

/// Updates what the editor integrations remember.
pub async fn update_editor_state<TEnvironment: Environment>(environment: &TEnvironment, update: impl FnOnce(&mut EditorState)) -> Result<()> {
  // prevent another process from storing something between the read and
  // write of the state file, which would lose one of the two
  let lock_dir = environment.get_cache_dir().join("locks");
  let _ignore = environment.mk_dir_all(&lock_dir);
  let _flag = LaxSingleProcessFsFlag::lock(
    environment,
    lock_dir.join(".editor-state.lock"),
    "Waiting for file lock for the editor state...",
  )
  .await;
  update_editor_state_without_lock(environment, update)
}

/// Reads what the editor integrations remember, which starts over when the
/// file can't be read or was written by a version with another format.
pub fn read_editor_state(environment: &impl Environment) -> EditorState {
  let read = |file_name: &str| {
    environment
      .maybe_read_file(environment.get_cache_dir().join(file_name))
      .ok()
      .flatten()
      .map(|text| serde_json::from_str::<EditorState>(&text).unwrap_or_default())
  };
  read(STATE_FILE_NAME).or_else(|| read(LEGACY_STATE_FILE_NAME)).unwrap_or_default()
}

pub(crate) fn update_editor_state_without_lock(environment: &impl Environment, update: impl FnOnce(&mut EditorState)) -> Result<()> {
  let mut state = read_editor_state(environment);
  update(&mut state);
  let file_path = get_editor_state_file_path(environment);
  if let Some(dir_path) = file_path.parent() {
    environment.mk_dir_all(dir_path)?;
  }
  environment.atomic_write_file_bytes(file_path, &serde_json::to_vec_pretty(&state)?)?;
  Ok(())
}

pub(crate) fn get_editor_state_file_path(environment: &impl Environment) -> PathBuf {
  environment.get_cache_dir().join(STATE_FILE_NAME)
}

fn add_process_plugin_decision(state: &mut EditorState, decision: ProcessPluginDecision) {
  let decisions = &mut state.process_plugin_decisions;
  // the decision for a previous configuration isn't needed anymore
  decisions.retain(|existing| existing.approval.config_file != decision.approval.config_file);
  decisions.push(decision);
  if decisions.len() > MAX_PROCESS_PLUGIN_DECISIONS {
    let excess = decisions.len() - MAX_PROCESS_PLUGIN_DECISIONS;
    decisions.drain(..excess);
  }
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use super::*;
  use crate::configuration::get_settings_file_path;
  use crate::environment::TestEnvironment;

  fn approval(config_file: &str, config_hash: &str) -> ProcessPluginApproval {
    ProcessPluginApproval {
      config_file: config_file.to_string(),
      config_hash: config_hash.to_string(),
    }
  }

  fn decision(config_file: &str, config_hash: &str, allowed: bool) -> ProcessPluginDecision {
    ProcessPluginDecision {
      approval: approval(config_file, config_hash),
      allowed,
      decided_at: 0,
    }
  }

  fn read_permissions(environment: &TestEnvironment, session_decisions: &[ProcessPluginDecision]) -> Permissions {
    let session_permissions = SessionPermissions::default();
    for decision in session_decisions {
      session_permissions.add_process_plugin_decision(decision.clone());
    }
    session_permissions.read(environment)
  }

  fn require_approval(environment: &TestEnvironment) {
    let file_path = get_settings_file_path(environment).unwrap();
    // macOS only uses this directory when it's already there
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment.write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": false } }"#).unwrap();
  }

  #[test]
  fn should_allow_process_plugins_by_default() {
    let environment = TestEnvironment::new();
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1")));
  }

  #[test]
  fn should_use_stored_and_session_decisions() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    for decision in [
      decision("/a/dprint.json", "1", true),
      decision("/b/dprint.json", "1", false),
      decision("/a/dprint.json", "2", true),
    ] {
      update_editor_state_without_lock(&environment, |state| add_process_plugin_decision(state, decision)).unwrap();
    }
    assert_eq!(
      read_editor_state(&environment).process_plugin_decisions,
      vec![decision("/b/dprint.json", "1", false), decision("/a/dprint.json", "2", true)]
    );

    let permissions = read_permissions(&environment, &[decision("/c/dprint.json", "1", true), decision("/a/dprint.json", "2", false)]);
    // the session decision wins
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "2")));
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "2")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1")));
    assert!(permissions.is_process_plugin_denied(&approval("/b/dprint.json", "1")));
    assert!(permissions.is_process_plugin_allowed(&approval("/c/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/d/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_denied(&approval("/d/dprint.json", "1")));
    assert!(permissions.was_allowed_with_other_config(&approval("/a/dprint.json", "3")));
    assert!(!permissions.was_allowed_with_other_config(&approval("/b/dprint.json", "3")));
  }

  #[test]
  fn should_use_the_most_recent_decision() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let stored = |allowed, decided_at| {
      update_editor_state_without_lock(&environment, |state| {
        add_process_plugin_decision(
          state,
          ProcessPluginDecision {
            decided_at,
            ..decision("/a/dprint.json", "1", allowed)
          },
        )
      })
      .unwrap();
    };
    let session_decision = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1", false)
    };

    // ex. allowing them with `dprint config editor-approve` after denying them for the session
    stored(true, 3);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    stored(true, 1);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_denied(&approval("/a/dprint.json", "1")));
  }

  #[test]
  fn should_record_when_the_decision_was_made() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(5);
    assert_eq!(ProcessPluginDecision::new(&environment, approval("/a/dprint.json", "1"), true).decided_at, 5000);
  }

  #[test]
  fn should_log_unknown_settings_once() {
    let environment = TestEnvironment::new();
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment
      .write_file(&file_path, r#"{ "editor": { "allowProcessPlugin": false }, "other": 1 }"#)
      .unwrap();
    let session_permissions = SessionPermissions::default();
    // the property isn't known, so process plugins are allowed by default
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1"))
    );
    session_permissions.read(&environment);
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        format!("Unknown property in {}: editor.allowProcessPlugin", file_path.display()),
        format!("Unknown property in {}: other", file_path.display()),
      ]
    );
  }

  #[test]
  fn should_forget_the_oldest_decisions() {
    let mut state = EditorState::default();
    for i in 0..MAX_PROCESS_PLUGIN_DECISIONS + 2 {
      add_process_plugin_decision(&mut state, decision(&format!("/{i}/dprint.json"), "1", true));
    }
    assert_eq!(state.process_plugin_decisions.len(), MAX_PROCESS_PLUGIN_DECISIONS);
    assert_eq!(state.process_plugin_decisions[0], decision("/2/dprint.json", "1", true));
  }

  #[test]
  fn should_not_allow_process_plugins_when_settings_do_not_parse() {
    let environment = TestEnvironment::new();
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment.write_file(&file_path, "{ nope").unwrap();
    let session_permissions = SessionPermissions::default();
    assert!(
      !session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1"))
    );
    // only logged once
    session_permissions.read(&environment);
    let messages = environment.take_stderr_messages();
    assert_eq!(messages.len(), 1);
    assert!(
      messages[0].starts_with("Requiring approval to run process plugins in editors because the settings couldn't be read: "),
      "{}",
      messages[0]
    );
  }

  #[test]
  fn should_read_the_legacy_state_file() {
    // dprint 0.60 to 0.64 stored it in lsp-state.json
    let environment = TestEnvironment::new();
    environment.mk_dir_all(environment.get_cache_dir()).unwrap();
    environment
      .write_file(
        environment.get_cache_dir().join(LEGACY_STATE_FILE_NAME),
        r#"{ "noConfigNotification": { "dismissed": true } }"#,
      )
      .unwrap();
    assert!(read_editor_state(&environment).no_config_notification.dismissed);
    update_editor_state_without_lock(&environment, |state| {
      state.no_config_notification.dismissed_workspace_folders.insert("/a".to_string());
    })
    .unwrap();
    let state = read_editor_state(&environment);
    assert!(state.no_config_notification.dismissed);
    assert!(environment.path_exists(get_editor_state_file_path(&environment)));
  }

  #[test]
  fn should_ignore_an_invalid_state_file() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all(environment.get_cache_dir()).unwrap();
    environment.write_file(get_editor_state_file_path(&environment), "not json").unwrap();
    assert!(!read_editor_state(&environment).no_config_notification.dismissed);
  }
}
