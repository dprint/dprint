use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use jsonc_parser::cst::CstObject;
use jsonc_parser::cst::CstRootNode;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;
use crate::utils::LaxSingleProcessFsFlag;

use super::get_init_config_file_text::to_cst_input;
use super::get_settings_file_path;
use super::read_settings;

/// Name of the file in the cache directory with what the editor integrations
/// (the language server and the editor service) remember, such as the user's
/// choices.
const STATE_FILE_NAME: &str = "editor-state.json";
/// Name of the file the language server used before (dprint 0.60 to 0.64).
const LEGACY_STATE_FILE_NAME: &str = "lsp-state.json";
/// Name of the file next to the settings file with the process plugin
/// decisions the user said to always use.
const APPROVALS_FILE_NAME: &str = "editor-approvals.json";
/// The version of the format of the approvals file that this version of dprint
/// reads and writes (see `EditorApprovals` for how it changes).
const APPROVALS_FILE_VERSION: u32 = 1;
/// The oldest version of the format whose readers may use what this version
/// of dprint writes, which is only raised by a change that an older reader
/// would apply too broadly (see `EditorApprovals`).
const APPROVALS_FILE_MIN_VERSION: u32 = 1;
/// The most process plugin decisions to remember, where the oldest are
/// forgotten first, which only means the user is asked again.
const MAX_PROCESS_PLUGIN_DECISIONS: usize = 5000;

/// What the editor integrations remember that's only for convenience.
///
/// It's in the cache directory because losing it only has the user notified
/// again. Clearing the cache also discards the list of workspace folders that
/// builds up in it.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorState {
  pub no_config_notification: NoConfigNotificationState,
}

/// The process plugin decisions the user said to always use, which are next
/// to the settings file because they're the user's decisions and not a cache
/// (ex. clearing the cache shouldn't forget what they denied).
///
/// Every version of dprint that's installed uses the same file (ex. a
/// project's npm dprint next to a global one), so it changes by these rules:
///
/// - `version` is the newest version of the format that wrote to the file, and
///   `minVersion` is the oldest version of the format whose readers may use it
///   (it's `version` when it's not there). A reader only refuses the file when
///   `minVersion` is greater than the version it supports, and otherwise
///   ignores what it doesn't know: properties and decisions it can't parse.
/// - A writer edits the file in place, so it keeps the properties and
///   decisions it doesn't know, and it never lowers `version` or `minVersion`.
///   It writes its decisions in its own format, so a reader needs to handle the
///   decisions of any version from `minVersion` on.
/// - A change only raises `minVersion` when a reader that ignores it would
///   allow too much (ex. a property that narrows what a decision allows). A
///   change that a reader can ignore (ex. a property that's only informational)
///   keeps it.
/// - A writer never writes over a file it refuses to read, because that would
///   lose what the user denied.
#[derive(Debug, Default)]
pub struct EditorApprovals {
  /// The decisions with the most recent last, which leaves out the ones in a
  /// format this version of dprint doesn't know.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
}

impl EditorApprovals {
  /// Remembers the decision for a process plugin of a config file, which
  /// replaces the one for a previous configuration of it.
  pub fn add_process_plugin_decision(&mut self, decision: ProcessPluginDecision) {
    let decisions = &mut self.process_plugin_decisions;
    // the decision for a previous configuration isn't needed anymore
    decisions.retain(|existing| existing.approval.config_file != decision.approval.config_file || existing.approval.plugin != decision.approval.plugin);
    decisions.push(decision);
    while decisions.len() > MAX_PROCESS_PLUGIN_DECISIONS {
      // forget the oldest decision that allowed a plugin first so that what
      // the user denied is still denied
      let index = decisions.iter().position(|decision| decision.allowed).unwrap_or(0);
      decisions.remove(index);
    }
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoConfigNotificationState {
  pub dismissed: bool,
  /// The lowercased paths of the folders, which are in a sorted set so that
  /// finding one doesn't go over every folder.
  pub dismissed_workspace_folders: BTreeSet<String>,
}

/// A process plugin of a config file with its configuration, which is what the
/// user allows or denies running.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginApproval {
  /// The config file that specifies the process plugin.
  pub config_file: String,
  /// The plugin as the config file specifies it (ex. its url and checksum).
  pub plugin: String,
  /// The SHA-256 checksum of the plugin's configuration in the config file,
  /// so a change to it (ex. the commands of dprint-plugin-exec) needs to be
  /// decided on again.
  ///
  /// It starts with the version of how it's made and a colon (ex. `1:` and
  /// the checksum in hex), so a version of dprint that makes it another way
  /// can tell it apart from a configuration that changed.
  ///
  /// It's `None` for the plugin with any configuration, which is only for
  /// denying it because its configuration isn't known until it's set up.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_hash: Option<String>,
}

impl ProcessPluginApproval {
  /// Creates what denying the process plugin of the config file with any
  /// configuration is decided on.
  pub fn any_config(config_file: String, plugin: String) -> Self {
    Self {
      config_file,
      plugin,
      config_hash: None,
    }
  }
}

/// Whether the user allowed or denied running a process plugin of a config
/// file with its configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginDecision {
  #[serde(flatten)]
  pub approval: ProcessPluginApproval,
  pub allowed: bool,
  /// When the user decided, in milliseconds since the Unix epoch, so that the
  /// most recent decision wins (ex. allowing it by running `dprint fmt` after
  /// denying it for the session).
  #[serde(default)]
  pub decided_at: u64,
}

impl ProcessPluginDecision {
  /// Creates the decision the user made now.
  pub fn new(environment: &impl Environment, approval: ProcessPluginApproval, allowed: bool) -> Self {
    Self {
      approval,
      allowed,
      decided_at: get_decided_at(environment),
    }
  }
}

/// The user allowing a process plugin of a config file to be set up for the
/// session, which runs it, before its configuration is known. Once it's set
/// up, it's allowed with its configuration (see `ProcessPluginApproval`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessPluginSetUpGrant {
  pub config_file: String,
  /// The plugin as the config file specifies it (ex. its url and checksum).
  pub plugin: String,
  /// The hash of the trusted configuration of the config file the user allowed
  /// it with, so that it's not allowed once that changes.
  pub trusted_config_hash: String,
  /// Whether to remember the decision once the plugin is set up instead of
  /// only using it for the session.
  pub always: bool,
  /// When the user allowed it, in milliseconds since the Unix epoch, so that
  /// denying it since (ex. in another window) wins.
  pub decided_at: u64,
}

impl ProcessPluginSetUpGrant {
  /// Creates what the user allowed now.
  pub fn new(environment: &impl Environment, config_file: String, plugin: String, trusted_config_hash: String, always: bool) -> Self {
    Self {
      config_file,
      plugin,
      trusted_config_hash,
      always,
      decided_at: get_decided_at(environment),
    }
  }
}

/// What the user allowed the editor integrations to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Permissions {
  /// Whether process plugins run without the user approving them.
  pub allow_process_plugins: Option<bool>,
  /// The decisions to always use followed by the ones for the session, where
  /// the most recent one for a plugin of a config file wins.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
  /// The process plugins the user allowed setting up for the session.
  pub process_plugin_set_up_grants: Vec<ProcessPluginSetUpGrant>,
  /// How many of `process_plugin_decisions` are the ones to always use.
  pub stored_process_plugin_decision_count: usize,
}

impl Permissions {
  /// Gets the permissions of the CLI, which runs every process plugin because
  /// the user ran it.
  pub fn allow_all() -> Self {
    Self {
      allow_process_plugins: Some(true),
      ..Default::default()
    }
  }

  pub fn is_process_plugin_allowed(&self, approval: &ProcessPluginApproval) -> bool {
    !self.requires_process_plugin_approval() || self.get_decision(approval) == Some(true)
  }

  /// Gets whether the process plugin of the config file may be set up with
  /// its trusted configuration, which runs it to get its information before
  /// its configuration is known.
  pub fn may_set_up_process_plugin(&self, config_file: &str, plugin: &str, trusted_config_hash: &str) -> bool {
    !self.requires_process_plugin_approval()
      || self.process_plugin_set_up_grants.iter().any(|grant| {
        grant.config_file == config_file && grant.plugin == plugin && grant.trusted_config_hash == trusted_config_hash && !self.is_denied_since(grant)
      })
  }

  /// Gets whether the user denied running the process plugin of the grant
  /// after allowing it to be set up (ex. "Always Deny" in another window).
  pub fn is_denied_since(&self, grant: &ProcessPluginSetUpGrant) -> bool {
    self.process_plugin_decisions.iter().any(|decision| {
      !decision.allowed
        && decision.approval.config_file == grant.config_file
        && decision.approval.plugin == grant.plugin
        && decision.decided_at > grant.decided_at
    })
  }

  /// Gets whether the user denied running the process plugin, which means
  /// they shouldn't be asked about it. Denying it with any configuration
  /// denies it with each one.
  pub fn is_process_plugin_denied(&self, approval: &ProcessPluginApproval) -> bool {
    self.get_decision(approval) == Some(false)
  }

  /// Gets whether the user chose to always deny running the process plugin
  /// (ex. "Always Deny") as opposed to only for the session.
  pub fn is_process_plugin_always_denied(&self, approval: &ProcessPluginApproval) -> bool {
    let stored_decisions = &self.process_plugin_decisions[..self.stored_process_plugin_decision_count.min(self.process_plugin_decisions.len())];
    self
      .get_winning_decision(approval)
      .is_some_and(|decision| !decision.allowed && stored_decisions.contains(decision))
  }

  /// Gets whether the process plugin of the config file was allowed when it
  /// had a different configuration.
  pub fn was_allowed_with_other_config(&self, approval: &ProcessPluginApproval) -> bool {
    self.process_plugin_decisions.iter().any(|decision| {
      decision.allowed
        && decision.approval.config_file == approval.config_file
        && decision.approval.plugin == approval.plugin
        && decision.approval.config_hash != approval.config_hash
        // a hash that's made another way (ex. by another version of dprint) doesn't say the configuration changed
        && get_config_hash_version(&decision.approval) == get_config_hash_version(approval)
    })
  }

  fn requires_process_plugin_approval(&self) -> bool {
    // IMPORTANT: Opening a project isn't the same as choosing to run its
    // process plugins, so they need to be approved unless the user opted out.
    self.allow_process_plugins != Some(true)
  }

  fn get_decision(&self, approval: &ProcessPluginApproval) -> Option<bool> {
    self.get_winning_decision(approval).map(|decision| decision.allowed)
  }

  fn get_winning_decision(&self, approval: &ProcessPluginApproval) -> Option<&ProcessPluginDecision> {
    self
      .process_plugin_decisions
      .iter()
      .filter(|decision| {
        decision.approval.config_file == approval.config_file
          && decision.approval.plugin == approval.plugin
          // IMPORTANT: Only allow it with the configuration that was allowed.
          && (decision.approval.config_hash == approval.config_hash || (decision.approval.config_hash.is_none() && !decision.allowed))
      })
      // the last one wins a tie, which is the one for the session
      .max_by_key(|decision| decision.decided_at)
  }
}

/// What an editor integration keeps for its session about what the user
/// allowed it to do, which is shared by its threads.
#[derive(Debug, Default)]
pub struct SessionPermissions {
  /// What the user decided about running process plugins for the session.
  process_plugin_decisions: Mutex<Vec<ProcessPluginDecision>>,
  /// The process plugins the user allowed setting up for the session.
  process_plugin_set_up_grants: Mutex<Vec<ProcessPluginSetUpGrant>>,
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
    let mut process_plugin_decisions = if allow_process_plugins != Some(true) {
      match read_editor_approvals(environment) {
        Ok(approvals) => approvals.process_plugin_decisions,
        Err(err) => {
          // IMPORTANT: Don't allow what may not have been allowed, but keep denying
          // what the user denied (ex. the file is from a newer version of dprint).
          self.log_settings_problem(environment, format!("Asking to allow process plugins again in editors. {:#}", err));
          read_editor_approval_denials(environment)
        }
      }
    } else {
      Vec::new()
    };
    let stored_process_plugin_decision_count = process_plugin_decisions.len();
    process_plugin_decisions.extend(self.process_plugin_decisions.lock().iter().cloned());
    Permissions {
      allow_process_plugins,
      stored_process_plugin_decision_count,
      process_plugin_decisions,
      process_plugin_set_up_grants: self.process_plugin_set_up_grants.lock().clone(),
    }
  }

  /// Uses the decision for the rest of the session.
  pub fn add_process_plugin_decision(&self, decision: ProcessPluginDecision) {
    self.process_plugin_decisions.lock().push(decision);
  }

  /// Allows setting up the process plugin for the session.
  /// It replaces the one for the plugin of the config file (ex. for a trusted
  /// configuration that changed since), which would otherwise be taken first.
  pub fn add_process_plugin_set_up_grant(&self, grant: ProcessPluginSetUpGrant) {
    let mut grants = self.process_plugin_set_up_grants.lock();
    grants.retain(|existing| existing.config_file != grant.config_file || existing.plugin != grant.plugin);
    grants.push(grant);
  }

  /// Gets what the user decided for the session about running process
  /// plugins, with the most recent last.
  pub fn session_process_plugin_decisions(&self) -> Vec<ProcessPluginDecision> {
    self.process_plugin_decisions.lock().clone()
  }

  /// Gets the process plugins the user allowed setting up for the session.
  pub fn process_plugin_set_up_grants(&self) -> Vec<ProcessPluginSetUpGrant> {
    self.process_plugin_set_up_grants.lock().clone()
  }

  /// Forgets what the user decided for the session about the process plugins
  /// of the config files.
  pub fn forget_process_plugin_decisions(&self, is_config_file: &impl Fn(&str) -> bool) {
    self
      .process_plugin_decisions
      .lock()
      .retain(|decision| !is_config_file(&decision.approval.config_file));
    self.process_plugin_set_up_grants.lock().retain(|grant| !is_config_file(&grant.config_file));
  }

  /// Takes the grant for setting up the process plugin of the config file,
  /// which isn't needed anymore once it's set up.
  pub fn take_process_plugin_set_up_grant(&self, config_file: &str, plugin: &str) -> Option<ProcessPluginSetUpGrant> {
    let mut grants = self.process_plugin_set_up_grants.lock();
    let index = grants.iter().position(|grant| grant.config_file == config_file && grant.plugin == plugin)?;
    Some(grants.remove(index))
  }

  fn log_settings_problem(&self, environment: &impl Environment, message: String) {
    if self.logged_settings_problems.lock().insert(message.clone()) {
      log_warn!(environment, "{}", message);
    }
  }
}

/// Gets the time of a decision the user made now, in milliseconds since the
/// Unix epoch.
fn get_decided_at(environment: &impl Environment) -> u64 {
  environment
    .sys_time_now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0)
}

/// Remembers the user's decision for running a process plugin of a config
/// file, which replaces the one for a previous configuration of it.
pub async fn store_process_plugin_decision(environment: &impl Environment, decision: ProcessPluginDecision) -> Result<()> {
  update_editor_approvals(environment, |approvals| approvals.add_process_plugin_decision(decision)).await
}

/// Updates the process plugin decisions the user said to always use.
pub async fn update_editor_approvals<TEnvironment: Environment>(environment: &TEnvironment, update: impl FnOnce(&mut EditorApprovals)) -> Result<()> {
  let _flag = lock_editor_approvals(environment).await;
  update_editor_approvals_without_lock(environment, update)
}

/// Forgets the stored decisions of the config files, which includes the ones
/// in a format this version doesn't know (ex. from a newer version of dprint)
/// so that forgetting them doesn't leave one that allows a plugin behind. A
/// decision like that without a config file this version can read is only
/// forgotten with every config file.
pub async fn forget_editor_approvals<TEnvironment: Environment>(environment: &TEnvironment, is_config_file: impl Fn(&str) -> bool) -> Result<()> {
  let _flag = lock_editor_approvals(environment).await;
  let Some(file_path) = get_editor_approvals_file_path(environment) else {
    return Ok(());
  };
  let is_forgotten = |value: &serde_json::Value| match value.get("configFile").and_then(|config_file| config_file.as_str()) {
    Some(config_file) => is_config_file(config_file),
    None => is_config_file(""),
  };
  // the file isn't written when there's nothing to forget (ex. it doesn't exist)
  match read_approvals_file(environment, &file_path)? {
    Some((_, file)) if file.process_plugin_decisions.iter().any(is_forgotten) => {}
    _ => return Ok(()),
  }
  edit_editor_approvals(
    environment,
    |approvals| {
      approvals
        .process_plugin_decisions
        .retain(|decision| !is_config_file(&decision.approval.config_file));
    },
    is_forgotten,
  )
}

/// Reads the process plugin decisions the user said to always use, which
/// fails when the file can't be read, doesn't parse, or is from a version of
/// dprint that this version can't use (see `EditorApprovals`).
pub fn read_editor_approvals(environment: &impl Environment) -> Result<EditorApprovals> {
  let Some(file_path) = get_editor_approvals_file_path(environment) else {
    return Ok(Default::default());
  };
  let Some((_, file)) = read_approvals_file(environment, &file_path)? else {
    return Ok(Default::default());
  };
  Ok(EditorApprovals {
    process_plugin_decisions: file.process_plugin_decisions.into_iter().filter_map(parse_process_plugin_decision).collect(),
  })
}

/// Reads the decisions that denied a process plugin from a file that can't be
/// read otherwise (ex. it's from a newer version of dprint), which is empty
/// when it doesn't parse.
pub(crate) fn read_editor_approval_denials(environment: &impl Environment) -> Vec<ProcessPluginDecision> {
  let Some(file_path) = get_editor_approvals_file_path(environment) else {
    return Vec::new();
  };
  let Ok(Some(text)) = environment.maybe_read_file(&file_path) else {
    return Vec::new();
  };
  let Ok(file) = serde_json::from_str::<ApprovalsFile>(&text) else {
    return Vec::new();
  };
  let decisions = file
    .process_plugin_decisions
    .into_iter()
    // a decision that's in another format is left out
    .filter_map(parse_process_plugin_decision)
    .collect::<Vec<_>>();
  // a denial that a later decision replaced (they're the most recent last) isn't used
  decisions
    .iter()
    .enumerate()
    .filter(|(index, decision)| {
      !decision.allowed
        && !decisions[index + 1..]
          .iter()
          .any(|later| later.approval.config_file == decision.approval.config_file && later.approval.plugin == decision.approval.plugin)
    })
    .map(|(_, decision)| decision.clone())
    .collect()
}

pub(crate) fn update_editor_approvals_without_lock(environment: &impl Environment, update: impl FnOnce(&mut EditorApprovals)) -> Result<()> {
  edit_editor_approvals(environment, update, |_| false)
}

/// Updates the decisions of the approvals file in place, which removes the
/// decisions in a format this version doesn't know that `remove_unknown` says to.
fn edit_editor_approvals(
  environment: &impl Environment,
  update: impl FnOnce(&mut EditorApprovals),
  remove_unknown: impl Fn(&serde_json::Value) -> bool,
) -> Result<()> {
  let Some(file_path) = get_editor_approvals_file_path(environment) else {
    bail!("Could not find the directory of the settings file to store the decision in.");
  };
  // IMPORTANT: Never write over a file that couldn't be read because that
  // would lose what the user denied.
  let (text, file) = read_approvals_file(environment, &file_path)?.unwrap_or_default();
  // the file is edited in place so that what this version doesn't know is kept
  let root = CstRootNode::parse(&text, &Default::default()).with_context(|| format!("Failed parsing {}.", file_path.display()))?;
  let root_obj = root.object_value_or_set();
  // never lower the versions because another version of dprint may have written what needs them
  set_number_property(&root_obj, "version", file.version.max(APPROVALS_FILE_VERSION), 0);
  set_number_property(&root_obj, "minVersion", file.min_version().max(APPROVALS_FILE_MIN_VERSION), 1);
  let decisions_array = root_obj.array_value_or_set("processPluginDecisions");
  let mut known_decisions = Vec::new();
  for element in decisions_array.elements() {
    // the file was already read as JSON, so its text is JSON
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&element.to_string()) else {
      continue;
    };
    match parse_process_plugin_decision(value.clone()) {
      Some(decision) => known_decisions.push((element, decision)),
      None if remove_unknown(&value) => element.remove(),
      None => {}
    }
  }
  let mut approvals = EditorApprovals {
    process_plugin_decisions: known_decisions.iter().map(|(_, decision)| decision.clone()).collect(),
  };
  update(&mut approvals);
  // keep the elements of the decisions that are still there, which keeps their
  // properties that this version doesn't know, and add the new ones
  let mut decisions = approvals.process_plugin_decisions.into_iter().peekable();
  for (element, decision) in known_decisions {
    if decisions.peek() == Some(&decision) {
      decisions.next();
    } else {
      element.remove();
    }
  }
  for decision in decisions {
    decisions_array.append(to_cst_input(serde_json::to_value(&decision)?));
    decisions_array.ensure_multiline();
  }
  let mut text = root.to_string();
  if !text.ends_with('\n') {
    text.push('\n');
  }
  if let Some(dir_path) = file_path.parent() {
    environment.mk_dir_all(dir_path)?;
  }
  environment.atomic_write_file_bytes(file_path, text.as_bytes())?;
  Ok(())
}

/// Prevents another process from storing something between the read and
/// write of the approvals file, which would lose one of the two.
async fn lock_editor_approvals<TEnvironment: Environment>(environment: &TEnvironment) -> LaxSingleProcessFsFlag<TEnvironment> {
  let lock_dir = environment.get_cache_dir().join("locks");
  let _ignore = environment.mk_dir_all(&lock_dir);
  LaxSingleProcessFsFlag::lock(
    environment,
    lock_dir.join(".editor-approvals.lock"),
    "Waiting for file lock for the editor approvals...",
  )
  .await
}

pub(crate) fn get_editor_approvals_file_path(environment: &impl Environment) -> Option<PathBuf> {
  get_settings_file_path(environment).map(|file_path| file_path.with_file_name(APPROVALS_FILE_NAME))
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

/// What's read from the approvals file, where the decisions are kept as they
/// are because one in a format this version of dprint doesn't know is left out.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalsFile {
  #[serde(default)]
  version: u32,
  #[serde(default)]
  min_version: Option<u32>,
  #[serde(default)]
  process_plugin_decisions: Vec<serde_json::Value>,
}

impl ApprovalsFile {
  /// Gets the oldest version of the format whose readers may use the file,
  /// which is its version when it doesn't say.
  fn min_version(&self) -> u32 {
    self.min_version.unwrap_or(self.version)
  }
}

/// Reads the text of the approvals file with what's parsed from it, which is
/// `None` when there's no file and fails when it's from a version of dprint
/// that this version can't use.
fn read_approvals_file(environment: &impl Environment, file_path: &Path) -> Result<Option<(String, ApprovalsFile)>> {
  let Some(text) = environment.maybe_read_file(file_path)? else {
    return Ok(None);
  };
  let file = serde_json::from_str::<ApprovalsFile>(&text).with_context(|| format!("Failed parsing {}.", file_path.display()))?;
  if file.min_version() > APPROVALS_FILE_VERSION {
    bail!("{} is from a newer version of dprint, so it can't be read.", file_path.display());
  }
  Ok(Some((text, file)))
}

/// Parses a decision of the approvals file, which is `None` when it's in a
/// format this version of dprint doesn't know.
fn parse_process_plugin_decision(value: serde_json::Value) -> Option<ProcessPluginDecision> {
  serde_json::from_value(value).ok()
}

/// Sets a number property of the object, which is inserted at the index when
/// it's not there.
fn set_number_property(obj: &CstObject, name: &str, value: u32, index: usize) {
  match obj.get(name) {
    Some(prop) => prop.set_value(value.into()),
    None => {
      obj.insert(index, name, value.into());
    }
  }
}

/// Gets the version of how the config hash of the approval was made (see
/// `ProcessPluginApproval::config_hash`).
fn get_config_hash_version(approval: &ProcessPluginApproval) -> Option<&str> {
  approval.config_hash.as_deref()?.split_once(':').map(|(version, _)| version)
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
      plugin: "https://plugins.dprint.dev/plugin.json@checksum".to_string(),
      config_hash: Some(config_hash.to_string()),
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
  fn should_require_approving_process_plugins_by_default() {
    let environment = TestEnvironment::new();
    let permissions = read_permissions(&environment, &[]);
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1")));
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", "plugin", "hash"));
    // the user can opt out
    crate::test_helpers::allow_process_plugins_in_editors(&environment);
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(permissions.may_set_up_process_plugin("/a/dprint.json", "plugin", "hash"));
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
      update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(decision)).unwrap();
    }
    assert_eq!(
      read_editor_approvals(&environment).unwrap().process_plugin_decisions,
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
      update_editor_approvals_without_lock(&environment, |approvals| {
        approvals.add_process_plugin_decision(ProcessPluginDecision {
          decided_at,
          ..decision("/a/dprint.json", "1", allowed)
        })
      })
      .unwrap();
    };
    let session_decision = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1", false)
    };

    // ex. allowing it by running `dprint fmt` after denying it for the session
    stored(true, 3);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    stored(true, 1);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_denied(&approval("/a/dprint.json", "1")));
  }

  #[test]
  fn should_deny_a_process_plugin_with_any_config() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let any_config = |config_file: &str, allowed| ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config(config_file.to_string(), approval(config_file, "1").plugin),
      allowed,
      decided_at: 1,
    };
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(any_config("/a/dprint.json", false))
    })
    .unwrap();
    // it's stored without a config hash
    let text = environment.read_file(get_editor_approvals_file_path(&environment).unwrap()).unwrap();
    assert!(!text.contains("configHash"), "{}", text);
    assert_eq!(
      read_editor_approvals(&environment).unwrap().process_plugin_decisions,
      vec![any_config("/a/dprint.json", false)]
    );

    let permissions = read_permissions(&environment, &[any_config("/b/dprint.json", true)]);
    let a_approval = ProcessPluginApproval::any_config("/a/dprint.json".to_string(), approval("/a/dprint.json", "1").plugin);
    assert!(permissions.is_process_plugin_denied(&a_approval));
    // it's denied with each configuration
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    // IMPORTANT: allowing it with any configuration doesn't allow it
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1")));
    // allowing it with its configuration afterwards wins (ex. running `dprint fmt`)
    let allowed = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1", true)
    };
    let permissions = read_permissions(&environment, &[allowed]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "2")));
  }

  #[test]
  fn should_keep_a_decision_for_each_plugin_of_a_config_file() {
    let mut state = EditorApprovals::default();
    let other_plugin = |config_hash: &str, allowed| {
      let mut decision = decision("/a/dprint.json", config_hash, allowed);
      decision.approval.plugin = "https://plugins.dprint.dev/other.json@checksum".to_string();
      decision
    };
    state.add_process_plugin_decision(decision("/a/dprint.json", "1", true));
    state.add_process_plugin_decision(other_plugin("1", true));
    state.add_process_plugin_decision(other_plugin("2", false));
    assert_eq!(
      state.process_plugin_decisions,
      vec![decision("/a/dprint.json", "1", true), other_plugin("2", false)]
    );
  }

  #[test]
  fn should_allow_setting_up_a_process_plugin_for_the_session() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let plugin = approval("/a/dprint.json", "1").plugin;
    assert!(
      !session_permissions
        .read(&environment)
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash")
    );
    session_permissions.add_process_plugin_set_up_grant(ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin: plugin.clone(),
      trusted_config_hash: "hash".to_string(),
      always: true,
      decided_at: 1,
    });
    let permissions = session_permissions.read(&environment);
    assert!(permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash"));
    assert!(!permissions.may_set_up_process_plugin("/b/dprint.json", &plugin, "hash"));
    // it's not allowed once the config file changes
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "other"));
    // it's not allowed with any configuration
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1")));
    assert!(session_permissions.take_process_plugin_set_up_grant("/a/dprint.json", &plugin).unwrap().always);
    assert!(
      !session_permissions
        .read(&environment)
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash")
    );
  }

  #[test]
  fn should_not_set_up_a_process_plugin_that_was_denied_since_it_was_allowed() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let plugin = approval("/a/dprint.json", "1").plugin;
    let grant = |decided_at| ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin: plugin.clone(),
      trusted_config_hash: "hash".to_string(),
      always: true,
      decided_at,
    };
    session_permissions.add_process_plugin_set_up_grant(grant(2));
    // denied before it was allowed
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
      allowed: false,
      decided_at: 1,
    });
    assert!(
      session_permissions
        .read(&environment)
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash")
    );
    // denied after (ex. in another window)
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
      allowed: false,
      decided_at: 3,
    });
    let permissions = session_permissions.read(&environment);
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash"));
    assert!(permissions.is_denied_since(&grant(2)));
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
    // the property isn't known, so process plugins need to be approved like by default
    assert!(
      !session_permissions
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
    let mut approvals = EditorApprovals::default();
    approvals.add_process_plugin_decision(decision("/denied/dprint.json", "1", false));
    for i in 0..MAX_PROCESS_PLUGIN_DECISIONS + 2 {
      approvals.add_process_plugin_decision(decision(&format!("/{i}/dprint.json"), "1", true));
    }
    assert_eq!(approvals.process_plugin_decisions.len(), MAX_PROCESS_PLUGIN_DECISIONS);
    // what was denied is kept
    assert_eq!(approvals.process_plugin_decisions[0], decision("/denied/dprint.json", "1", false));
    assert_eq!(approvals.process_plugin_decisions[1], decision("/3/dprint.json", "1", true));
  }

  #[test]
  fn should_not_write_over_approvals_that_do_not_parse() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint with another format
    environment.write_file(&file_path, r#"{ "version": 2, "other": [] }"#).unwrap();
    let session_permissions = SessionPermissions::default();
    session_permissions.add_process_plugin_decision(decision("/a/dprint.json", "1", true));
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1"))
    );
    assert_eq!(
      environment.take_stderr_messages(),
      vec![format!(
        "Asking to allow process plugins again in editors. {} is from a newer version of dprint, so it can't be read.",
        file_path.display()
      )]
    );
    assert!(
      update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(decision(
        "/b/dprint.json",
        "1",
        true
      )))
      .is_err()
    );
    assert_eq!(environment.read_file(&file_path).unwrap(), r#"{ "version": 2, "other": [] }"#);
    // and it's not in the cache directory, so clearing the cache doesn't forget it
    assert!(!file_path.starts_with(environment.get_cache_dir()));
  }

  #[test]
  fn should_write_the_approvals_file() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(decision("/a/dprint.json", "1:a", true))
    })
    .unwrap();
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "version": 1,
  "minVersion": 1,
  "processPluginDecisions": [
    {
      "configFile": "/a/dprint.json",
      "plugin": "https://plugins.dprint.dev/plugin.json@checksum",
      "configHash": "1:a",
      "allowed": true,
      "decidedAt": 0
    }
  ]
}
"#
    );
    update_editor_approvals_without_lock(&environment, |approvals| approvals.process_plugin_decisions.clear()).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "version": 1,
  "minVersion": 1,
  "processPluginDecisions": []
}
"#
    );
  }

  #[test]
  fn should_keep_what_it_does_not_know_when_writing_approvals() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint added properties and a kind of decision that this version can ignore
    environment
      .write_file(
        &file_path,
        r#"{
  "version": 3,
  "minVersion": 1,
  "other": { "a": 1 },
  "processPluginDecisions": [
    { "kind": "other", "allowed": false },
    {
      "configFile": "/a/dprint.json",
      "plugin": "https://plugins.dprint.dev/plugin.json@checksum",
      "configHash": "1:a",
      "allowed": false,
      "decidedAt": 0,
      "reason": "user"
    },
    {
      "configFile": "/b/dprint.json",
      "plugin": "https://plugins.dprint.dev/plugin.json@checksum",
      "configHash": "1:b",
      "allowed": true,
      "decidedAt": 0
    }
  ]
}
"#,
      )
      .unwrap();
    // it's read with what this version doesn't know left out
    assert_eq!(
      read_editor_approvals(&environment).unwrap().process_plugin_decisions,
      vec![decision("/a/dprint.json", "1:a", false), decision("/b/dprint.json", "1:b", true)]
    );
    assert!(read_permissions(&environment, &[]).is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));

    // replacing a decision keeps the rest of the file
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(decision("/b/dprint.json", "1:c", true))
    })
    .unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "version": 3,
  "minVersion": 1,
  "other": { "a": 1 },
  "processPluginDecisions": [
    { "kind": "other", "allowed": false },
    {
      "configFile": "/a/dprint.json",
      "plugin": "https://plugins.dprint.dev/plugin.json@checksum",
      "configHash": "1:a",
      "allowed": false,
      "decidedAt": 0,
      "reason": "user"
    },
    {
      "configFile": "/b/dprint.json",
      "plugin": "https://plugins.dprint.dev/plugin.json@checksum",
      "configHash": "1:c",
      "allowed": true,
      "decidedAt": 0
    }
  ]
}
"#
    );
  }

  #[test]
  fn should_forget_the_decisions_in_a_format_this_version_does_not_know() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    environment
      .write_file(
        &file_path,
        r#"{
  "version": 3,
  "minVersion": 1,
  "processPluginDecisions": [
    { "kind": "other", "allowed": true },
    { "configFile": "/a/dprint.json", "kind": "other", "allowed": true },
    { "configFile": "/b/dprint.json", "kind": "other", "allowed": true }
  ]
}
"#,
      )
      .unwrap();
    environment.clone().run_in_runtime(async move {
      // the one of the config file
      forget_editor_approvals(&environment, |config_file| config_file == "/a/dprint.json")
        .await
        .unwrap();
      let text = environment.read_file(&file_path).unwrap();
      assert!(!text.contains("/a/dprint.json"), "{}", text);
      assert!(text.contains("/b/dprint.json"), "{}", text);
      assert!(text.contains(r#"{ "kind": "other", "allowed": true }"#), "{}", text);
      // and every one
      forget_editor_approvals(&environment, |_| true).await.unwrap();
      let text = environment.read_file(&file_path).unwrap();
      assert!(!text.contains("allowed"), "{}", text);
    });
  }

  #[test]
  fn should_not_lower_the_versions_of_the_approvals_file() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. written before minVersion, which is its version
    environment.write_file(&file_path, r#"{ "version": 1, "processPluginDecisions": [] }"#).unwrap();
    update_editor_approvals_without_lock(&environment, |_| {}).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      r#"{
  "version": 1,
  "minVersion": 1,
  "processPluginDecisions": []
}
"#
    );
    // ex. a newer version of dprint wrote it
    environment
      .write_file(&file_path, r#"{ "minVersion": 1, "version": 2, "processPluginDecisions": [] }"#)
      .unwrap();
    update_editor_approvals_without_lock(&environment, |_| {}).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      "{ \"minVersion\": 1, \"version\": 2, \"processPluginDecisions\": [] }\n"
    );
  }

  #[test]
  fn should_not_use_approvals_with_a_newer_min_version() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint narrowed what a decision allows
    let text = r#"{
  "version": 2,
  "minVersion": 2,
  "processPluginDecisions": [
    { "configFile": "/a/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:a", "allowed": true, "decidedAt": 0 },
    { "configFile": "/b/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:b", "allowed": false, "decidedAt": 0 }
  ]
}"#;
    environment.write_file(&file_path, text).unwrap();
    let err = read_editor_approvals(&environment).unwrap_err();
    assert_eq!(
      err.to_string(),
      format!("{} is from a newer version of dprint, so it can't be read.", file_path.display())
    );
    let permissions = read_permissions(&environment, &[]);
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // what was denied is still denied
    assert!(permissions.is_process_plugin_denied(&approval("/b/dprint.json", "1:b")));
    assert_eq!(environment.take_stderr_messages().len(), 1);
    assert!(
      update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(decision(
        "/c/dprint.json",
        "1:c",
        true
      )))
      .is_err()
    );
    assert_eq!(environment.read_file(&file_path).unwrap(), text);
  }

  #[test]
  fn should_not_say_the_config_changed_for_a_hash_made_another_way() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    // ex. a version of dprint that hashes the configuration another way allowed it
    let permissions = read_permissions(
      &environment,
      &[decision("/a/dprint.json", "2:a", true), decision("/b/dprint.json", "1:a", true)],
    );
    assert!(!permissions.was_allowed_with_other_config(&approval("/a/dprint.json", "1:b")));
    assert!(permissions.was_allowed_with_other_config(&approval("/b/dprint.json", "1:b")));
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
