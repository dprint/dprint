use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use jsonc_parser::cst::CstNode;
use jsonc_parser::cst::CstObject;
use jsonc_parser::cst::CstRootNode;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;
use tokio::sync::Notify;

use crate::environment::Environment;
use crate::environment::FileLock;
use crate::resolution::PROCESS_PLUGIN_CONFIG_HASH_VERSION;
use crate::utils::LaxSingleProcessFsFlag;
use crate::utils::get_sha256_checksum;

use super::Settings;
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
/// can't safely ignore (see `EditorApprovals`).
const APPROVALS_FILE_MIN_VERSION: u32 = 1;
/// The property of the approvals file with the process plugin decisions.
const PROCESS_PLUGIN_DECISIONS_PROPERTY: &str = "processPluginDecisions";
/// The most process plugin decisions to remember, where the oldest are
/// forgotten first, which only means the user is asked again.
const MAX_PROCESS_PLUGIN_DECISIONS: usize = 5000;
/// Name of the file next to the approvals file that every writer of it locks
/// (see `EditorApprovals`).
const APPROVALS_LOCK_FILE_NAME: &str = ".editor-approvals.lock";
/// How long a writer of the approvals file waits for its lock before it fails
/// without writing (see `lock_editor_approvals`).
const APPROVALS_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a writer of the approvals file waits before it tries to take the
/// lock again.
const APPROVALS_LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
/// The fingerprint of the approvals file when there's none (see
/// `ApprovalsFileRead::fingerprint`).
const NO_APPROVALS_FILE_FINGERPRINT: &str = "none";

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
///   (it's `version` when it's not there). Both stay integers, and a reader
///   treats one that isn't an integer it can read as newer than the version
///   it supports.
/// - An addition doesn't raise `minVersion` (ex. a property of a decision or
///   of the file, a kind of decision, or another way of making the
///   `configHash`). A reader whose version is at least `minVersion` ignores
///   what it doesn't know: properties and decisions it can't parse. A writer
///   edits the file in place, so it keeps them, and it never lowers `version`
///   or `minVersion`.
/// - `minVersion` is only raised for a change that an older reader can't
///   safely ignore (ex. a property of a decision that narrows what an allow
///   covers). It can't narrow what an older version does without asking (ex.
///   downloading from a trusted domain), because a reader that refuses the
///   file still trusts the domains of the settings file, so a decision like
///   that is an `editor` setting of the settings file instead, which an older
///   version that doesn't know it fails closed for (see `Settings`).
/// - A reader whose version is lower than `minVersion` refuses the file. It
///   still applies the denials in it that it can parse, because denying more
///   than the user did is safe, but it ignores the allows and never writes to
///   the file, because it doesn't know what its changes mean. An "Always"
///   decision is then only used for the session, and forgetting decisions
///   leaves the ones in the file, which the user is told to update dprint for.
///   So `processPluginDecisions` stays an array even when `minVersion` is
///   raised, and a decision that denies a process plugin is always also
///   written in the format of version 1 (ex. without a `configHash` to deny it
///   with any configuration).
/// - A writer writes its decisions in its own format, so a reader needs to
///   handle the decisions of any version from `minVersion` on. It removes the
///   decisions in an older format for what it decides about (ex. the same
///   plugin of the same config file), so an older reader doesn't keep using
///   one the user replaced, apart from the allows whose `configHash` is made
///   another way (see below).
/// - A `configHash` starts with the version of how it's made (see
///   `ProcessPluginApproval::config_hash`), which changes without a
///   `minVersion` because a reader never matches a hash it doesn't make. A
///   reader only uses an allow whose hash is made the way it makes them, so an
///   allow it can't check never hides an older one it can, and it still applies
///   a denial with any hash. An allow is stored for each way its hash is made:
///   a writer that stores an allow keeps the allows for the same plugin whose
///   hash is made another way and only removes those with a denial, so two
///   versions of dprint that make it differently don't undo each other's
///   allows. A writer that can make an older hash too may store an allow with
///   each, so the older readers use it.
/// - When decisions for the same plugin are compared by when they were made, a
///   denial wins a tie with an allow.
/// - The file stays strict JSON (no comments or trailing commas), because a
///   reader parses it as JSON (see `ApprovalsFile::parse`), and one that
///   can't parse it loses its denials (it asks again about what the user
///   always denied) and can't store anything.
/// - Every writer takes the same lock for the whole read and write of the
///   file: an exclusive lock of the whole `.editor-approvals.lock` file in the
///   file's directory, which is `flock(2)` on Unix and `LockFileEx` of the
///   whole file on Windows (see `Environment::try_lock_file`), tried again
///   until a timeout (see `lock_editor_approvals`). It never writes without
///   it: one that can't take it (ex. another process holds it for too long, or
///   the file system doesn't support locks) fails, which only keeps the
///   decision for the session. A writer that uses another lock
///   (ex. an `fcntl` lock, which doesn't exclude a `flock` lock on Linux) or
///   skips it would race with the others, which could lose an "Always Deny"
///   under an "Always Allow" written at the same time. No version deletes or
///   replaces `.editor-approvals.lock`, because a version isn't required to
///   check that the path still names the file it locked, so a writer that
///   opened a new one wouldn't be excluded.
/// - Decisions are only in the top-level arrays whose names end with
///   `Decisions` (ex. `processPluginDecisions`), and each one is an object with
///   a top-level `configFile` string in the form of `PathSource::display` (ex.
///   `/home/user/project/dprint.json`), so that a version that doesn't know
///   their format can forget them.
/// - IMPORTANT: How the `configFile` of a local config file is made is part of
///   the format. It's the canonical path of the directory it's used for joined
///   with the names dprint looked up (ex. `.config/dprint.json`), which is the
///   path it was found at when that isn't its canonical path (ex. a symlink, or
///   those names in another case on disk, see `get_config_file_found_path`),
///   and was its canonical path in older versions. A version that changes how
///   it's made stores a denial under each older form it can make too, and
///   removes the allows under them when it stores a denial or forgets, like
///   what's stored with another config hash version (see
///   `PROCESS_PLUGIN_CONFIG_HASH_VERSION`), and it forgets a decision by the
///   `configFile` text it lists for it too. Forgetting the decisions of config files
///   removes the ones with their `configFile` from each of those arrays, and
///   forgetting every decision empties each of them, including the decisions
///   in a format it doesn't know. The other top-level properties don't have
///   decisions.
#[derive(Debug, Default)]
pub struct EditorApprovals {
  /// The decisions with the most recent last, which leaves out the ones in a
  /// format this version of dprint doesn't know.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
}

impl EditorApprovals {
  /// Remembers the decision for a process plugin of a config file, which
  /// replaces the one for a previous configuration of it. It isn't remembered
  /// when the decision for the plugin is more recent, whether it allows or
  /// denies it, because a decision may be stored after it was made (ex. once
  /// the plugin the user allowed is set up). A denial wins a tie.
  ///
  /// An allow keeps the allows whose config hash is made another way (ex. by
  /// another version of dprint), which a denial removes (see `EditorApprovals`).
  pub fn add_process_plugin_decision(&mut self, decision: ProcessPluginDecision) {
    let decisions = &mut self.process_plugin_decisions;
    // the decisions this one replaces, which keep it from being stored when they're more recent
    let is_replaced = |existing: &ProcessPluginDecision| {
      existing.approval.config_file == decision.approval.config_file
        && existing.approval.plugin == decision.approval.plugin
        && (!decision.allowed || !existing.allowed || get_config_hash_version(&existing.approval) == get_config_hash_version(&decision.approval))
    };
    // IMPORTANT: Don't replace what the user decided since (ex. "Always Deny" in another
    // window, or running `dprint fmt` with a changed configuration, while the plugin they
    // allowed was being set up).
    if decisions.iter().any(|existing| is_replaced(existing) && is_more_recent(existing, &decision)) {
      return;
    }
    // the decision for a previous configuration isn't needed anymore
    decisions.retain(|existing| !is_replaced(existing));
    decisions.push(decision);
    while decisions.len() > MAX_PROCESS_PLUGIN_DECISIONS {
      // forget the oldest decision that allowed a plugin first so that what
      // the user denied is still denied
      let index = decisions.iter().position(|decision| decision.allowed).unwrap_or(0);
      decisions.remove(index);
    }
  }

  /// Gets whether the decisions allow running the process plugin with its
  /// configuration like they do in an editor, which an allow whose config hash
  /// is made another way doesn't change.
  pub fn is_process_plugin_allowed(&self, approval: &ProcessPluginApproval) -> bool {
    get_winning_decision(&self.process_plugin_decisions, approval).is_some_and(|decision| decision.allowed)
  }
}

/// The approvals file has a `minVersion` that's newer than the version of the
/// format this version of dprint supports, so this version only applies the
/// denials in it and never writes to it (see `EditorApprovals`). Storing or
/// forgetting a decision fails with it, so the user can be told to update
/// dprint.
#[derive(Debug)]
pub struct NewerApprovalsFileError {
  pub file_path: PathBuf,
}

impl std::fmt::Display for NewerApprovalsFileError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{} is from a newer version of dprint, so it can't be read.", self.file_path.display())
  }
}

impl std::error::Error for NewerApprovalsFileError {}

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
///
/// It's also kept in the session file (see `SessionFile`), where another
/// version of dprint may read it, so a change to it is a change to that
/// format too.
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
  /// The key of the plugin's configuration in the config file, which is the
  /// configuration that's hashed. It's used to find the configuration an
  /// allow is for before the plugin is set up (ex. by another version of
  /// dprint or with another cache directory), which is checked with the
  /// plugin's own key once it's set up, so a reader that ignores it (ex. it
  /// was stored before it existed) can try the configuration of each key.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_key: Option<String>,
}

impl ProcessPluginApproval {
  /// Creates what denying the process plugin of the config file with any
  /// configuration is decided on.
  pub fn any_config(config_file: String, plugin: String) -> Self {
    Self {
      config_file,
      plugin,
      config_hash: None,
      config_key: None,
    }
  }
}

/// Whether the user allowed or denied running a process plugin of a config
/// file with its configuration.
///
/// It's also kept in the session file (see `SessionFile`), where another
/// version of dprint may read it, so a change to it is a change to that
/// format too. A change that raises the approvals file's
/// `minVersion` (see `EditorApprovals`) raises `SESSION_FILE_VERSION` too.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
  /// Creates the decision the user made now, which is more recent than the
  /// decisions for the plugin (see `get_decided_now_at`).
  pub fn new(environment: &impl Environment, approval: ProcessPluginApproval, allowed: bool, decisions: &[ProcessPluginDecision]) -> Self {
    let decided_at = get_decided_now_at(environment, decisions, &approval.config_file, &approval.plugin);
    Self { approval, allowed, decided_at }
  }

  /// Gets whether this version of dprint uses the decision, which is a denial
  /// or an allow whose config hash is made the way this version makes it (see
  /// `EditorApprovals`). An allow it can't check is left out so that it
  /// doesn't hide an older one it can.
  pub fn is_used(&self) -> bool {
    !self.allowed || get_config_hash_version(&self.approval).is_some_and(|version| version == PROCESS_PLUGIN_CONFIG_HASH_VERSION.to_string())
  }
}

/// The user allowing a process plugin of a config file to be set up for the
/// session, which runs it, before its configuration is known. Once it's set
/// up, it's allowed with its configuration (see `ProcessPluginApproval`).
///
/// It's also kept in the session file (see `SessionFile`), where another
/// version of dprint may read it, so a change to it is a change to that
/// format too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginSetUpGrant {
  pub config_file: String,
  /// The plugin as the config file specifies it (ex. its url and checksum).
  pub plugin: String,
  /// The hash of the trusted configuration of the config file the user allowed
  /// it with, so that it's not allowed once that changes.
  pub trusted_config_hash: String,
  /// The checksum of the plugin's file when the config file doesn't pin it
  /// (an npm plugin without a version), which the user allowed like the
  /// approval id says, so that it's not allowed once that changes (ex. the
  /// package was updated).
  pub unpinned_file_checksum: Option<String>,
  /// Whether to remember the decision once the plugin is set up instead of
  /// only using it for the session.
  pub always: bool,
  /// When the user allowed it, in milliseconds since the Unix epoch, so that
  /// denying it since (ex. in another window) wins.
  pub decided_at: u64,
}

impl ProcessPluginSetUpGrant {
  /// Creates what the user allowed now, which is more recent than the
  /// decisions for the plugin (see `get_decided_now_at`).
  pub fn new(
    environment: &impl Environment,
    config_file: String,
    plugin: String,
    trusted_config_hash: String,
    unpinned_file_checksum: Option<String>,
    always: bool,
    decisions: &[ProcessPluginDecision],
  ) -> Self {
    let decided_at = get_decided_now_at(environment, decisions, &config_file, &plugin);
    Self {
      config_file,
      plugin,
      trusted_config_hash,
      unpinned_file_checksum,
      always,
      decided_at,
    }
  }
}

/// What the user allowed the editor integrations to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Permissions {
  /// Whether process plugins run without the user approving them.
  pub allow_process_plugins: Option<bool>,
  /// Why the editors don't use the settings file when they don't (see
  /// `read_ignored_settings_message`), which the messages that ask the user
  /// say because it may be why they're asked.
  pub ignored_settings_message: Option<String>,
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
  /// its trusted configuration and the checksum of its file when the config
  /// file doesn't pin it, which runs it to get its information before its
  /// configuration is known.
  pub fn may_set_up_process_plugin(&self, config_file: &str, plugin: &str, trusted_config_hash: &str, unpinned_file_checksum: Option<&str>) -> bool {
    !self.requires_process_plugin_approval()
      || self.process_plugin_set_up_grants.iter().any(|grant| {
        grant.config_file == config_file
          && grant.plugin == plugin
          && grant.trusted_config_hash == trusted_config_hash
          && grant.unpinned_file_checksum.as_deref() == unpinned_file_checksum
          && !self.is_denied_since(grant)
      })
  }

  /// Gets whether the user denied running the process plugin of the grant
  /// after allowing it to be set up (ex. "Always Deny" in another window),
  /// which includes a denial at the same time because a denial wins a tie.
  pub fn is_denied_since(&self, grant: &ProcessPluginSetUpGrant) -> bool {
    self.process_plugin_decisions.iter().any(|decision| {
      !decision.allowed
        && decision.approval.config_file == grant.config_file
        && decision.approval.plugin == grant.plugin
        && is_more_recent_at(decision.decided_at, false, grant.decided_at, true)
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

  /// Gets whether the most recent decision for the process plugin of the
  /// config file allowed it when it had a different configuration. An allow
  /// whose hash is made another way (ex. by another version of dprint) isn't
  /// used, so it doesn't say the configuration changed.
  pub fn was_allowed_with_other_config(&self, approval: &ProcessPluginApproval) -> bool {
    get_latest_decision(&self.process_plugin_decisions, &approval.config_file, &approval.plugin)
      .is_some_and(|decision| decision.allowed && decision.approval.config_hash != approval.config_hash)
  }

  /// Gets what the most recent decision for the process plugin of the config
  /// file allowed when it allows it, which is for a configuration of the
  /// plugin that may not be known yet (ex. it isn't set up).
  pub fn get_latest_allow(&self, config_file: &str, plugin: &str) -> Option<&ProcessPluginApproval> {
    get_latest_decision(&self.process_plugin_decisions, config_file, plugin)
      .filter(|decision| decision.allowed)
      .map(|decision| &decision.approval)
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
    get_winning_decision(&self.process_plugin_decisions, approval)
  }
}

/// What an editor integration keeps for its session about what the user
/// allowed it to do, which is shared by its threads.
///
/// A decision for the session is compared with the stored ones by when they
/// were made, except that a denial that's stored after a decision for the
/// session was made always wins over it, because the time of a stored
/// decision may be before it although it was made after it (ex. it's from a
/// machine whose clock is behind, or the decision from the future that the
/// time of the session's decision is after was forgotten since). A stored
/// denial that none of the reads of the permissions saw before removes the
/// session's allows and set-up grants for the plugin (see `read`). That's
/// usually before the user decides again, but it's also one that was stored
/// (ex. by another window) after the last read and before the user's
/// decision, which then loses to it, which is the side to err on.
#[derive(Debug, Default)]
pub struct SessionPermissions {
  /// What the user decided about running process plugins for the session.
  process_plugin_decisions: Mutex<Vec<ProcessPluginDecision>>,
  /// The stored denials that the reads of the permissions saw, which tells
  /// apart the ones that were stored since.
  stored_denial_reads: Mutex<StoredDenialReads>,
  /// The process plugins the user allowed setting up for the session.
  process_plugin_set_up_grants: Mutex<Vec<ProcessPluginSetUpGrant>>,
  /// The failures to store what the user always allowed for the process
  /// plugins of a config file once they were set up, which an approval that
  /// set them up takes to tell the user.
  process_plugin_store_errors: Mutex<ProcessPluginStoreErrors>,
  /// The problems with the settings that were logged, which are only logged
  /// once because the settings are read for each request.
  logged_settings_problems: Mutex<HashSet<String>>,
  /// Notified when what's in `SessionPermissionsState` changes (see `changed`).
  changes: Notify,
  /// Held to read while a decision is stored and to write while the stored
  /// decisions are forgotten (see `pause_stores`).
  stores: tokio::sync::RwLock<()>,
}

/// What the session decided about process plugins, which is what an editor
/// integration keeps in its session file so that a server that's started
/// again for the same session behaves as if it never stopped.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionPermissionsState {
  /// What the user decided for the session with the most recent last, which
  /// includes an "Always" decision that couldn't be stored.
  pub process_plugin_decisions: Vec<ProcessPluginDecision>,
  pub process_plugin_set_up_grants: Vec<ProcessPluginSetUpGrant>,
  /// The stored denials the session saw, so a denial that was stored since
  /// is still told apart from them (see `SessionPermissions`).
  pub seen_stored_denials: Vec<ProcessPluginDecision>,
  /// The fingerprint of the approvals file the last time the session read it
  /// (see `ApprovalsFileRead::fingerprint`), which is `None` when it's not
  /// known (ex. reading it failed). Restoring the state leaves out its allows
  /// when the file changed since (see `SessionPermissions::restore`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub approvals_file_fingerprint: Option<String>,
}

/// The stored denials that the reads of the permissions saw (see
/// `SessionPermissions::remove_allows_denied_since`).
#[derive(Debug, Default)]
struct StoredDenialReads {
  /// How many reads started, which numbers them in the order they started
  /// because the threads may finish them in another order.
  started: u64,
  /// The number of the last read that started of the ones that were applied.
  applied: u64,
  /// The stored denials that the reads saw, which are only added to because a
  /// denial that's stored again is another one (its time is another), so a
  /// read that fails or is out of date doesn't make one look new again.
  denials: HashSet<ProcessPluginDecision>,
  /// The fingerprint of the approvals file of the last read that was applied,
  /// which is `None` when it's not known (ex. the last read failed).
  approvals_file_fingerprint: Option<String>,
}

/// Where the failures to store what the user always allowed for process
/// plugins start for something (see `SessionPermissions::process_plugin_store_error_mark`).
#[derive(Debug, Clone, Copy)]
pub struct ProcessPluginStoreErrorMark(u64);

/// The failures to store what the user always allowed for the process plugins
/// of a config file once they were set up.
#[derive(Debug, Default)]
struct ProcessPluginStoreErrors {
  /// How many failures there were, which numbers them.
  count: u64,
  /// The number and message of the last failure of each config file.
  by_config_file: HashMap<String, (u64, String)>,
}

impl SessionPermissions {
  /// Reads what the user allowed the editor integrations to do, along with
  /// what they decided for the session.
  pub fn read(&self, environment: &impl Environment) -> Permissions {
    let settings = read_settings(environment);
    if let Ok(Some(settings)) = &settings {
      let file_path = get_settings_file_path(environment).map(|path| path.display().to_string()).unwrap_or_default();
      for property_name in &settings.unknown_properties {
        self.log_settings_problem(environment, format!("Unknown property in {}: {}", file_path, property_name));
      }
    }
    let ignored_reason = get_ignored_settings_reason(environment, &settings);
    let allow_process_plugins = match (&settings, &ignored_reason) {
      (_, Some(reason)) => {
        // IMPORTANT: Don't allow what the user may have tried to disallow, and a property or
        // value this version doesn't know may be a setting of a newer version that narrows
        // what's allowed, so fail closed to the narrowest state (see `Settings`), which the
        // trusted domains do too.
        self.log_settings_problem(
          environment,
          format!(
            "Requiring approval to run process plugins and to download from any domain in editors, and not using the stored \"Always Allow\" decisions, because {}.",
            reason
          ),
        );
        Some(false)
      }
      (Ok(Some(settings)), None) => settings.editor.allow_process_plugins,
      (_, None) => None,
    };
    // only read the decisions when they're used
    let mut process_plugin_decisions = if allow_process_plugins != Some(true) {
      let read_number = self.start_stored_decisions_read();
      let read = ApprovalsFileRead::read(environment);
      let approvals = match &read {
        Ok(Some(read)) => read.approvals(),
        Ok(None) => Ok(Default::default()),
        Err(err) => Err(anyhow::anyhow!("{:#}", err)),
      };
      let decisions = match approvals {
        // IMPORTANT: The settings may have a setting of a newer version that the stored allows
        // shouldn't be used with (ex. one that never allows process plugins), so only use the
        // denials, like with a file this version can't read (see `Settings`).
        Ok(approvals) if ignored_reason.is_some() => Some(get_unreplaced_denials(&approvals.process_plugin_decisions)),
        Ok(approvals) => Some(approvals.process_plugin_decisions),
        Err(err) => {
          // IMPORTANT: Don't allow what may not have been allowed, but keep denying
          // what the user denied (ex. the file is from a newer version of dprint).
          self.log_settings_problem(environment, format!("Asking to allow process plugins again in editors. {:#}", err));
          match &read {
            Ok(Some(read)) => read.denials(),
            Ok(None) => Some(Vec::new()),
            Err(_) => None,
          }
        }
      };
      match decisions {
        Some(decisions) => {
          let fingerprint = read
            .ok()
            .map(|read| read.map_or_else(|| NO_APPROVALS_FILE_FINGERPRINT.to_string(), |read| read.fingerprint));
          self.remove_allows_denied_since(read_number, &decisions, fingerprint);
          decisions
        }
        None => {
          self.forget_approvals_file_fingerprint();
          Vec::new()
        }
      }
    } else {
      Vec::new()
    };
    let ignored_settings_message = ignored_reason.map(|reason| get_ignored_settings_message(environment, &reason));
    let stored_process_plugin_decision_count = process_plugin_decisions.len();
    process_plugin_decisions.extend(self.process_plugin_decisions.lock().iter().cloned());
    Permissions {
      allow_process_plugins,
      ignored_settings_message,
      stored_process_plugin_decision_count,
      process_plugin_decisions,
      process_plugin_set_up_grants: self.process_plugin_set_up_grants.lock().clone(),
    }
  }

  /// Uses the decision for the rest of the session.
  pub fn add_process_plugin_decision(&self, decision: ProcessPluginDecision) {
    self.process_plugin_decisions.lock().push(decision);
    self.changes.notify_one();
  }

  /// Allows setting up the process plugin for the session.
  /// It replaces the one for the plugin of the config file (ex. for a trusted
  /// configuration that changed since), which would otherwise be taken first.
  pub fn add_process_plugin_set_up_grant(&self, grant: ProcessPluginSetUpGrant) {
    let mut grants = self.process_plugin_set_up_grants.lock();
    grants.retain(|existing| existing.config_file != grant.config_file || existing.plugin != grant.plugin);
    grants.push(grant);
    self.changes.notify_one();
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
    self.changes.notify_one();
  }

  /// Gets what the session decided about process plugins (ex. to keep it in
  /// a session file).
  pub fn state(&self) -> SessionPermissionsState {
    let mut seen_stored_denials = self.stored_denial_reads.lock().denials.iter().cloned().collect::<Vec<_>>();
    // the same state is the same text
    seen_stored_denials
      .sort_by(|a, b| (a.decided_at, &a.approval.config_file, &a.approval.plugin).cmp(&(b.decided_at, &b.approval.config_file, &b.approval.plugin)));
    SessionPermissionsState {
      process_plugin_decisions: self.process_plugin_decisions.lock().clone(),
      process_plugin_set_up_grants: self.process_plugin_set_up_grants.lock().clone(),
      seen_stored_denials,
      approvals_file_fingerprint: self.stored_denial_reads.lock().approvals_file_fingerprint.clone(),
    }
  }

  /// Uses what a session decided about process plugins before (ex. what a
  /// server that stopped kept in the session file), which is done before
  /// anything is decided.
  ///
  /// The allows and set-up grants are left out when the approvals file
  /// changed since the session last read it, or that isn't known, because a
  /// stored decision that replaced them may have been removed since (ex. an
  /// "Always Deny" in another window that was then forgotten while the server
  /// was stopped), which a server that kept running would have seen. That
  /// only means the user is asked again. The denials are kept.
  pub fn restore(&self, environment: &impl Environment, mut state: SessionPermissionsState) {
    let fingerprint = read_editor_approvals_fingerprint(environment);
    if state.approvals_file_fingerprint.is_none() || state.approvals_file_fingerprint != fingerprint {
      state.process_plugin_decisions.retain(|decision| !decision.allowed);
      state.process_plugin_set_up_grants.clear();
    }
    *self.process_plugin_decisions.lock() = state.process_plugin_decisions;
    *self.process_plugin_set_up_grants.lock() = state.process_plugin_set_up_grants;
    let mut reads = self.stored_denial_reads.lock();
    reads.denials = state.seen_stored_denials.into_iter().collect();
    reads.approvals_file_fingerprint = fingerprint;
  }

  /// Marks that a decision the user always uses is being stored until the
  /// guard is dropped, which waits while stores are paused (see
  /// `pause_stores`).
  pub async fn start_store(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
    self.stores.read().await
  }

  /// Waits for the decisions that are being stored to be stored and keeps
  /// others from being stored until the guard is dropped, so forgetting the
  /// stored decisions forgets one the user made before (ex. one that waited
  /// for the approvals file's lock).
  pub async fn pause_stores(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
    self.stores.write().await
  }

  /// Waits until what's in `SessionPermissionsState` changed since the last
  /// time this returned, which returns right away when it changed already.
  pub async fn changed(&self) {
    self.changes.notified().await;
  }

  /// Marks where the failures to store what the user always allowed for
  /// process plugins start for something that may set them up (ex. an
  /// approval), so it only takes the ones that happen after it (see
  /// `take_process_plugin_store_error`).
  pub fn process_plugin_store_error_mark(&self) -> ProcessPluginStoreErrorMark {
    ProcessPluginStoreErrorMark(self.process_plugin_store_errors.lock().count)
  }

  /// Remembers that storing what the user always allowed for the process
  /// plugins of the config file failed once they were set up (see
  /// `record_set_up_process_plugins`), which replaces the previous failure.
  pub fn set_process_plugin_store_error(&self, config_file: String, message: String) {
    let mut errors = self.process_plugin_store_errors.lock();
    errors.count += 1;
    let number = errors.count;
    errors.by_config_file.insert(config_file, (number, message));
  }

  /// Forgets the failure to store what the user always allowed for the
  /// process plugins of the config file (ex. storing it succeeded since).
  pub fn clear_process_plugin_store_error(&self, config_file: &str) {
    self.process_plugin_store_errors.lock().by_config_file.remove(config_file);
  }

  /// Takes the message of the last failure to store what the user always
  /// allowed for the process plugins of the config file once they were set up
  /// when it happened after the mark. One from before is forgotten because it
  /// isn't about what was done since (ex. it's from setting them up for
  /// another request a while ago).
  pub fn take_process_plugin_store_error(&self, config_file: &str, mark: ProcessPluginStoreErrorMark) -> Option<String> {
    let (number, message) = self.process_plugin_store_errors.lock().by_config_file.remove(config_file)?;
    (number > mark.0).then_some(message)
  }

  /// Takes the grant for setting up the process plugin of the config file,
  /// which isn't needed anymore once it's set up.
  pub fn take_process_plugin_set_up_grant(&self, config_file: &str, plugin: &str) -> Option<ProcessPluginSetUpGrant> {
    let mut grants = self.process_plugin_set_up_grants.lock();
    let index = grants.iter().position(|grant| grant.config_file == config_file && grant.plugin == plugin)?;
    self.changes.notify_one();
    Some(grants.remove(index))
  }

  /// Numbers a read of the stored decisions, which is done before reading
  /// them (see `remove_allows_denied_since`).
  fn start_stored_decisions_read(&self) -> u64 {
    let mut reads = self.stored_denial_reads.lock();
    reads.started += 1;
    reads.started
  }

  /// Removes the session's allows and set-up grants for the process plugins
  /// that a denial was stored for that no read saw before (see
  /// `SessionPermissions`), from the stored decisions that a read that
  /// succeeded got.
  ///
  /// A read that started before one that was already applied is out of date,
  /// so a denial only it has may have been replaced since (ex. the user
  /// forgot it or always allowed the plugin), which is only remembered. The
  /// fingerprint of the approvals file the read got is kept when it's applied.
  fn remove_allows_denied_since(&self, read_number: u64, stored_decisions: &[ProcessPluginDecision], approvals_file_fingerprint: Option<String>) {
    let stored_denials = stored_decisions.iter().filter(|decision| !decision.allowed).cloned().collect::<HashSet<_>>();
    let mut reads = self.stored_denial_reads.lock();
    let is_out_of_date = read_number < reads.applied;
    let new_denials = stored_denials.difference(&reads.denials).collect::<Vec<_>>();
    if !is_out_of_date && !new_denials.is_empty() {
      let is_denied = |config_file: &str, plugin: &str| {
        new_denials
          .iter()
          .any(|denial| denial.approval.config_file == config_file && denial.approval.plugin == plugin)
      };
      // IMPORTANT: The denial is more recent than them whatever its time says.
      self
        .process_plugin_decisions
        .lock()
        .retain(|decision| !decision.allowed || !is_denied(&decision.approval.config_file, &decision.approval.plugin));
      self
        .process_plugin_set_up_grants
        .lock()
        .retain(|grant| !is_denied(&grant.config_file, &grant.plugin));
    }
    let mut is_changed = false;
    if !is_out_of_date {
      reads.applied = read_number;
      if reads.approvals_file_fingerprint != approvals_file_fingerprint {
        reads.approvals_file_fingerprint = approvals_file_fingerprint;
        is_changed = true;
      }
    }
    let seen_count = reads.denials.len();
    reads.denials.extend(stored_denials);
    if is_changed || reads.denials.len() != seen_count {
      self.changes.notify_one();
    }
  }

  /// Forgets the fingerprint of the approvals file after reading it failed,
  /// so restoring the session's state doesn't use its allows (see `restore`).
  fn forget_approvals_file_fingerprint(&self) {
    if self.stored_denial_reads.lock().approvals_file_fingerprint.take().is_some() {
      self.changes.notify_one();
    }
  }

  fn log_settings_problem(&self, environment: &impl Environment, message: String) {
    if self.logged_settings_problems.lock().insert(message.clone()) {
      log_warn!(environment, "{}", message);
    }
  }
}

/// Gets whether what was decided at a time is more recent than what was
/// decided at another like `is_more_recent` (ex. for a set-up grant).
pub fn is_more_recent_at(decided_at: u64, allowed: bool, other_decided_at: u64, other_allowed: bool) -> bool {
  decided_at > other_decided_at || (decided_at == other_decided_at && !allowed && other_allowed)
}

/// Reads why the editors don't use the settings file, which is a message for
/// the user that's `None` when they use it. They don't when it can't be read
/// or `editor` has a property or a value that this version doesn't know (see
/// `Settings`), and then trust no domains and ask before running process
/// plugins without using the stored allows.
pub fn read_ignored_settings_message(environment: &impl Environment) -> Option<String> {
  let reason = get_ignored_settings_reason(environment, &read_settings(environment))?;
  Some(get_ignored_settings_message(environment, &reason))
}

/// Gets the time of a decision the user made now about the process plugin of
/// the config file, in milliseconds since the Unix epoch. It's after each of
/// the decisions for the plugin, so that what the user decided is used even
/// when one of them seems to be from the future (ex. it's from a machine whose
/// clock is ahead, or the clock was set back since).
fn get_decided_now_at(environment: &impl Environment, decisions: &[ProcessPluginDecision], config_file: &str, plugin: &str) -> u64 {
  let now = environment
    .sys_time_now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0);
  decisions
    .iter()
    .filter(|decision| decision.approval.config_file == config_file && decision.approval.plugin == plugin)
    .map(|decision| decision.decided_at.saturating_add(1))
    .fold(now, u64::max)
}

/// Remembers the user's decision for running a process plugin of a config
/// file, which replaces the one for a previous configuration of it.
pub async fn store_process_plugin_decision(environment: &impl Environment, decision: ProcessPluginDecision) -> Result<()> {
  update_editor_approvals(environment, |approvals| approvals.add_process_plugin_decision(decision)).await
}

/// Updates the process plugin decisions the user said to always use.
///
/// It fails without writing when the approvals file's lock can't be taken
/// (see `lock_editor_approvals`).
pub async fn update_editor_approvals<TEnvironment: Environment>(environment: &TEnvironment, update: impl FnOnce(&mut EditorApprovals)) -> Result<()> {
  let _lock = lock_editor_approvals(environment).await?;
  update_editor_approvals_without_lock(environment, update)
}

/// Forgets the stored decisions of the config files, which includes the ones
/// in a format this version doesn't know (ex. from a newer version of dprint)
/// and the ones of other kinds (see `EditorApprovals`) so that forgetting them
/// doesn't leave one that allows something behind. A decision like that
/// without a config file this version can read is only forgotten with every
/// config file.
///
/// It fails with `NewerApprovalsFileError` without changing a file that this
/// version refuses to read when it has decisions to forget, and without
/// changing it when its lock can't be taken (see `lock_editor_approvals`).
pub async fn forget_editor_approvals<TEnvironment: Environment>(environment: &TEnvironment, is_config_file: impl Fn(&str) -> bool) -> Result<()> {
  let Some(file_path) = get_editor_approvals_file_path(environment) else {
    return Ok(());
  };
  let is_forgotten = |value: &serde_json::Value| match value.get("configFile").and_then(|config_file| config_file.as_str()) {
    Some(config_file) => is_config_file(config_file),
    None => is_config_file(""),
  };
  // the lock isn't taken when there's nothing to forget (ex. there's no file), so forgetting
  // doesn't fail then when it can't be taken
  if !read_approvals_file(environment, &file_path)?.is_some_and(|(_, file)| file.decisions().any(is_forgotten)) {
    return Ok(());
  }
  let _lock = lock_editor_approvals(environment).await?;
  // the file isn't written when there's nothing to forget anymore (ex. another process forgot them)
  match read_approvals_file(environment, &file_path)? {
    Some((_, file)) if file.decisions().any(is_forgotten) => file.ensure_readable(&file_path)?,
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
/// dprint that this version can't use (`NewerApprovalsFileError`).
pub fn read_editor_approvals(environment: &impl Environment) -> Result<EditorApprovals> {
  match ApprovalsFileRead::read(environment)? {
    Some(read) => read.approvals(),
    None => Ok(Default::default()),
  }
}

/// Reads the fingerprint of the approvals file (see
/// `ApprovalsFileRead::fingerprint`), which is `None` when it can't be read.
pub fn read_editor_approvals_fingerprint(environment: &impl Environment) -> Option<String> {
  match ApprovalsFileRead::read(environment) {
    Ok(Some(read)) => Some(read.fingerprint),
    Ok(None) => Some(NO_APPROVALS_FILE_FINGERPRINT.to_string()),
    Err(_) => None,
  }
}

/// Reads the decisions that denied a process plugin from a file that can't be
/// read otherwise (ex. it's from a newer version of dprint), which is empty
/// when it doesn't parse. Only `processPluginDecisions` needs to parse (see
/// `EditorApprovals`).
pub(crate) fn read_editor_approval_denials(environment: &impl Environment) -> Vec<ProcessPluginDecision> {
  try_read_editor_approval_denials(environment).unwrap_or_default()
}

/// Reads the denials like `read_editor_approval_denials`, which is `None`
/// when the file can't be read or doesn't parse.
fn try_read_editor_approval_denials(environment: &impl Environment) -> Option<Vec<ProcessPluginDecision>> {
  match ApprovalsFileRead::read(environment) {
    Ok(Some(read)) => read.denials(),
    Ok(None) => Some(Vec::new()),
    Err(_) => None,
  }
}

/// Gets the stored decisions that deny a process plugin, apart from the ones
/// that a more recent decision replaced, which is a later one on a tie.
fn get_unreplaced_denials(decisions: &[ProcessPluginDecision]) -> Vec<ProcessPluginDecision> {
  decisions
    .iter()
    .enumerate()
    .filter(|(index, decision)| {
      !decision.allowed
        && !decisions.iter().enumerate().any(|(later_index, later)| {
          later.approval.config_file == decision.approval.config_file
            && later.approval.plugin == decision.approval.plugin
            && (later.decided_at > decision.decided_at || (later.decided_at == decision.decided_at && later_index > *index))
        })
    })
    .map(|(_, decision)| decision.clone())
    .collect()
}

pub(crate) fn update_editor_approvals_without_lock(environment: &impl Environment, update: impl FnOnce(&mut EditorApprovals)) -> Result<()> {
  edit_editor_approvals(environment, update, |_| false)
}

/// Updates the decisions of the approvals file in place, which removes the
/// decisions in a format this version doesn't know, or of another kind, that
/// `remove_unknown` says to.
///
/// It fails with `NewerApprovalsFileError` without writing anything to a file
/// that this version refuses to read (see `EditorApprovals`).
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
  // IMPORTANT: Never write to a file from a newer version that this version can't use because
  // it doesn't know what its changes mean (ex. a newer version may apply an allow in the format
  // of this version too broadly).
  file.ensure_readable(&file_path)?;
  // the file is edited in place so that what this version doesn't know is kept
  let root = CstRootNode::parse(&text, &Default::default()).with_context(|| format!("Failed parsing {}.", file_path.display()))?;
  let root_obj = root.object_value_or_set();
  // never lower the versions because another version of dprint may have written what needs them
  raise_version_property(&root_obj, "version", APPROVALS_FILE_VERSION, 0);
  raise_version_property(&root_obj, "minVersion", APPROVALS_FILE_MIN_VERSION, 1);
  remove_other_decisions(&root_obj, &remove_unknown);
  let decisions_array = root_obj.array_value_or_set(PROCESS_PLUGIN_DECISIONS_PROPERTY);
  let mut known_decisions = Vec::new();
  for element in decisions_array.elements() {
    let Some(value) = to_json_value(&element) else {
      continue;
    };
    match parse_process_plugin_decision(value.clone()) {
      Some(decision) => known_decisions.push((element, decision)),
      None if remove_unknown(&value) => element.remove(),
      // a decision this version doesn't know is kept as it is
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
/// write of the approvals file, which would lose one of the two. The lock is
/// next to the file instead of in the cache directory so that each dprint
/// that uses the file uses it, even with another cache directory (ex.
/// `DPRINT_CACHE_DIR`).
///
/// IMPORTANT: Every version of dprint takes this lock and never writes the
/// file without it (see `EditorApprovals`), so this fails when it can't be
/// taken within `APPROVALS_LOCK_TIMEOUT` (ex. another process holds it) or at
/// all (ex. the file system doesn't support it), and the caller doesn't write.
async fn lock_editor_approvals(environment: &impl Environment) -> Result<FileLock> {
  lock_editor_approvals_within(environment, APPROVALS_LOCK_TIMEOUT).await
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
#[derive(Debug)]
struct ApprovalsFile {
  /// The oldest version of the format whose readers may use the file, which
  /// is its version when it doesn't say, and is `None` when it isn't an
  /// integer this version can read, which means it's newer (see
  /// `EditorApprovals`).
  min_version: Option<u64>,
  process_plugin_decisions: Vec<serde_json::Value>,
  /// The other properties, which may have decisions of other kinds.
  other: serde_json::Map<String, serde_json::Value>,
}

impl Default for ApprovalsFile {
  /// Gets what an approvals file that isn't there is like.
  fn default() -> Self {
    Self {
      min_version: Some(0),
      process_plugin_decisions: Vec::new(),
      other: Default::default(),
    }
  }
}

impl ApprovalsFile {
  /// Parses the approvals file, which only fails when it isn't an object or
  /// `processPluginDecisions` isn't an array, so that a reader that refuses it
  /// still finds the denials in it (see `EditorApprovals`).
  fn parse(text: &str) -> Result<Self> {
    let serde_json::Value::Object(mut obj) = serde_json::from_str::<serde_json::Value>(text)? else {
      bail!("Expected an object.");
    };
    let version = match obj.remove("version") {
      Some(version) => version.as_u64(),
      None => Some(0),
    };
    let min_version = match obj.remove("minVersion") {
      Some(min_version) => min_version.as_u64(),
      None => version,
    };
    let process_plugin_decisions = match obj.remove(PROCESS_PLUGIN_DECISIONS_PROPERTY) {
      Some(serde_json::Value::Array(decisions)) => decisions,
      Some(_) => bail!("Expected \"{}\" to be an array.", PROCESS_PLUGIN_DECISIONS_PROPERTY),
      None => Vec::new(),
    };
    Ok(Self {
      min_version,
      process_plugin_decisions,
      other: obj,
    })
  }

  /// Fails with `NewerApprovalsFileError` when this version of dprint refuses
  /// to read the file, which is when its `minVersion` is newer than the
  /// version this version supports.
  fn ensure_readable(&self, file_path: &Path) -> Result<()> {
    if self.min_version.is_none_or(|min_version| min_version > APPROVALS_FILE_VERSION as u64) {
      return Err(
        NewerApprovalsFileError {
          file_path: file_path.to_path_buf(),
        }
        .into(),
      );
    }
    Ok(())
  }

  /// Gets each decision in the file as it is, which includes the ones of the
  /// kinds this version doesn't know (see `EditorApprovals`). A property for
  /// decisions that isn't an array is a decision.
  fn decisions(&self) -> impl Iterator<Item = &serde_json::Value> {
    let other_decisions = self
      .other
      .iter()
      .filter(|(name, _)| is_decisions_property(name))
      .flat_map(|(_, value)| match value {
        serde_json::Value::Array(elements) => elements.as_slice(),
        value => std::slice::from_ref(value),
      });
    self.process_plugin_decisions.iter().chain(other_decisions)
  }
}

/// A read of the approvals file, which what's parsed from it and its
/// fingerprint are from.
struct ApprovalsFileRead {
  file_path: PathBuf,
  /// The text of the file, which is `None` when there's no file.
  text: Option<String>,
  /// What identifies this version of the file, which is its modification time
  /// in nanoseconds since the Unix epoch and the SHA-256 checksum of its text
  /// (`<nanoseconds>:<checksum>`), or `none` when there's no file. The time
  /// tells apart a file that was changed and changed back (ex. a decision that
  /// was stored and forgotten). It's only compared with fingerprints from the
  /// same machine.
  fingerprint: String,
}

impl ApprovalsFileRead {
  /// Reads the approvals file, which is `None` when there's no settings
  /// directory for it.
  fn read(environment: &impl Environment) -> std::io::Result<Option<Self>> {
    let Some(file_path) = get_editor_approvals_file_path(environment) else {
      return Ok(None);
    };
    // the time is read before the text, so a change in between makes the fingerprint not match a later one
    let modified = environment.fs_metadata(&file_path).and_then(|metadata| metadata.modified()).ok();
    let text = environment.maybe_read_file(&file_path)?;
    let fingerprint = match &text {
      Some(text) => format!("{}:{}", get_unix_nanos(modified), get_sha256_checksum(text.as_bytes())),
      None => NO_APPROVALS_FILE_FINGERPRINT.to_string(),
    };
    Ok(Some(Self { file_path, text, fingerprint }))
  }

  /// Gets the decisions of the file, which fails when it doesn't parse or is
  /// from a version of dprint that this version can't use.
  fn approvals(&self) -> Result<EditorApprovals> {
    let Some(file) = self.parse()? else {
      return Ok(Default::default());
    };
    file.ensure_readable(&self.file_path)?;
    Ok(EditorApprovals {
      process_plugin_decisions: file.process_plugin_decisions.into_iter().filter_map(parse_process_plugin_decision).collect(),
    })
  }

  /// Gets the decisions that deny a process plugin like
  /// `read_editor_approval_denials`, which is `None` when the file doesn't
  /// parse.
  fn denials(&self) -> Option<Vec<ProcessPluginDecision>> {
    let Ok(file) = self.parse() else {
      return None;
    };
    let decisions = file
      .map(|file| file.process_plugin_decisions)
      .unwrap_or_default()
      .into_iter()
      // a decision that's in another format is left out
      .filter_map(parse_process_plugin_decision)
      .collect::<Vec<_>>();
    Some(get_unreplaced_denials(&decisions))
  }

  fn parse(&self) -> Result<Option<ApprovalsFile>> {
    let Some(text) = &self.text else {
      return Ok(None);
    };
    let file = ApprovalsFile::parse(text).with_context(|| format!("Failed parsing {}.", self.file_path.display()))?;
    Ok(Some(file))
  }
}

/// Takes the lock of the approvals file like `lock_editor_approvals`, which
/// fails when it can't be taken within the timeout.
async fn lock_editor_approvals_within(environment: &impl Environment, timeout: Duration) -> Result<FileLock> {
  let lock_path = get_editor_approvals_lock_path(environment);
  if let Some(dir_path) = lock_path.parent() {
    environment
      .mk_dir_all(dir_path)
      .with_context(|| format!("Failed creating {}.", dir_path.display()))?;
  }
  let start = std::time::Instant::now();
  let mut progress = None;
  loop {
    match environment.try_lock_file(&lock_path) {
      Ok(Some(lock)) => return Ok(lock),
      Ok(None) => {}
      Err(err) => return Err(err).with_context(|| format!("Failed locking {}.", lock_path.display())),
    }
    let elapsed = start.elapsed();
    if elapsed >= timeout {
      bail!("Timed out waiting for another dprint process to release the lock {}.", lock_path.display());
    }
    if progress.is_none() && elapsed >= Duration::from_secs(1) {
      progress = environment.progress_bars().map(|progress_bars| {
        progress_bars.add_progress(
          "Waiting for file lock for the editor approvals...".to_string(),
          crate::utils::ProgressBarStyle::Action,
          1,
        )
      });
    }
    tokio::time::sleep(APPROVALS_LOCK_RETRY_DELAY).await;
  }
}

/// Gets the path of the lock of the approvals file (see `EditorApprovals`).
fn get_editor_approvals_lock_path(environment: &impl Environment) -> PathBuf {
  let lock_dir = match get_editor_approvals_file_path(environment).as_deref().and_then(|file_path| file_path.parent()) {
    Some(dir_path) => dir_path.to_path_buf(),
    None => environment.get_cache_dir().join("locks"),
  };
  lock_dir.join(APPROVALS_LOCK_FILE_NAME)
}

/// Gets a time in nanoseconds since the Unix epoch, which is negative before
/// it and empty when it's not known.
fn get_unix_nanos(time: Option<SystemTime>) -> String {
  match time.map(|time| time.duration_since(UNIX_EPOCH)) {
    Some(Ok(duration)) => duration.as_nanos().to_string(),
    Some(Err(err)) => format!("-{}", err.duration().as_nanos()),
    None => String::new(),
  }
}

/// Reads the text of the approvals file with what's parsed from it, which is
/// `None` when there's no file. It doesn't check whether this version can use
/// it (see `ApprovalsFile::ensure_readable`).
fn read_approvals_file(environment: &impl Environment, file_path: &Path) -> Result<Option<(String, ApprovalsFile)>> {
  let Some(text) = environment.maybe_read_file(file_path)? else {
    return Ok(None);
  };
  let file = ApprovalsFile::parse(&text).with_context(|| format!("Failed parsing {}.", file_path.display()))?;
  Ok(Some((text, file)))
}

/// Parses a process plugin decision of the approvals file, which is `None`
/// when it's in a format this version doesn't know. The properties this
/// version doesn't know are ignored (see `EditorApprovals`).
fn parse_process_plugin_decision(value: serde_json::Value) -> Option<ProcessPluginDecision> {
  serde_json::from_value(value).ok()
}

/// Removes the decisions of the kinds other than process plugin decisions
/// that `remove` says to, which are in a format this version doesn't know
/// (see `EditorApprovals`).
fn remove_other_decisions(root_obj: &CstObject, remove: &impl Fn(&serde_json::Value) -> bool) {
  for prop in root_obj.properties() {
    let Some(name) = prop.name().and_then(|name| name.decoded_value().ok()) else {
      continue;
    };
    if name == PROCESS_PLUGIN_DECISIONS_PROPERTY || !is_decisions_property(&name) {
      continue;
    }
    let Some(value) = prop.value() else {
      continue;
    };
    match value.as_array() {
      Some(array) => {
        for element in array.elements() {
          if to_json_value(&element).is_some_and(|value| remove(&value)) {
            element.remove();
          }
        }
      }
      // a property for decisions that isn't an array is a decision
      None => {
        if to_json_value(&value).is_some_and(|value| remove(&value)) {
          prop.remove();
        }
      }
    }
  }
}

/// Gets whether a top-level property of the approvals file has decisions
/// (see `EditorApprovals`).
fn is_decisions_property(name: &str) -> bool {
  name.ends_with("Decisions")
}

/// Gets the value of a node of the approvals file, which was already read as
/// JSON, so its text is JSON.
fn to_json_value(node: &CstNode) -> Option<serde_json::Value> {
  serde_json::from_str(&node.to_string()).ok()
}

/// Raises a version property of the object to the value when it's lower,
/// which is inserted at the index when it's not there. One that isn't an
/// integer this version can read is newer, so it's kept as it is.
fn raise_version_property(obj: &CstObject, name: &str, value: u32, index: usize) {
  match obj.get(name) {
    Some(prop) => {
      let current = prop.value().and_then(|value| to_json_value(&value)).and_then(|value| value.as_u64());
      if current.is_some_and(|current| current < value as u64) {
        prop.set_value(value.into());
      }
    }
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

/// Gets the message for the user that says why the editors don't use the
/// settings file (see `read_ignored_settings_message`).
fn get_ignored_settings_message(environment: &impl Environment, reason: &str) -> String {
  format!(
    "dprint {} doesn't use its settings file in editors because {}. Update dprint or change the file to use it.",
    environment.cli_version(),
    reason
  )
}

/// Gets why the editors don't use the settings that were read, which is said
/// after "because".
fn get_ignored_settings_reason(environment: &impl Environment, settings: &Result<Option<Settings>>) -> Option<String> {
  match settings {
    Ok(Some(settings)) => {
      let problem = settings.get_editor_problem()?;
      let file_path = get_settings_file_path(environment).map(|path| path.display().to_string()).unwrap_or_default();
      Some(format!("{} {}", file_path, problem))
    }
    Ok(None) => None,
    Err(err) => Some(format!("the settings couldn't be read: {:#}", err)),
  }
}

/// Gets whether a decision is more recent than another one for the same
/// process plugin, which a denial is at the same time as an allow because
/// denying more than the user did is safe (ex. two windows decide at the same
/// time after a decision from the future, see `get_decided_now_at`).
fn is_more_recent(decision: &ProcessPluginDecision, other: &ProcessPluginDecision) -> bool {
  is_more_recent_at(decision.decided_at, decision.allowed, other.decided_at, other.allowed)
}

/// Gets the decision that's used for the approval, which is the most recent
/// one for the process plugin of the config file when it's for the approval's
/// configuration or denies the plugin with any configuration. When the most
/// recent one is for another configuration, there's no decision, so the user
/// is asked instead of an older one being used (ex. a denial from before the
/// plugin was allowed with its previous configuration).
fn get_winning_decision<'a>(decisions: &'a [ProcessPluginDecision], approval: &ProcessPluginApproval) -> Option<&'a ProcessPluginDecision> {
  let decision = get_latest_decision(decisions, &approval.config_file, &approval.plugin)?;
  let is_for_approval = match decision.allowed {
    // IMPORTANT: Only allow it with the configuration that was allowed.
    true => decision.approval.config_hash.is_some() && decision.approval.config_hash == approval.config_hash,
    false => decision.approval.config_hash.is_none() || decision.approval.config_hash == approval.config_hash,
  };
  is_for_approval.then_some(decision)
}

/// Gets the most recent decision that this version uses (see
/// `ProcessPluginDecision::is_used`) for the process plugin of the config file
/// with any configuration. A denial wins a tie, and otherwise the last one
/// does, which is the one for the session.
fn get_latest_decision<'a>(decisions: &'a [ProcessPluginDecision], config_file: &str, plugin: &str) -> Option<&'a ProcessPluginDecision> {
  decisions
    .iter()
    .filter(|decision| decision.approval.config_file == config_file && decision.approval.plugin == plugin && decision.is_used())
    .max_by_key(|decision| (decision.decided_at, !decision.allowed))
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
      config_key: None,
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
    // the user decides after the permissions are read
    session_permissions.read(environment);
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
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", "plugin", "hash", None));
    // the user can opt out
    crate::test_helpers::allow_process_plugins_in_editors(&environment);
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(permissions.may_set_up_process_plugin("/a/dprint.json", "plugin", "hash", None));
  }

  #[test]
  fn should_only_take_a_failure_to_store_a_decision_that_happened_after_the_mark() {
    let session_permissions = SessionPermissions::default();
    // ex. from setting up the process plugins for another request a while ago
    session_permissions.set_process_plugin_store_error("/a/dprint.json".to_string(), "old".to_string());
    let mark = session_permissions.process_plugin_store_error_mark();
    assert_eq!(session_permissions.take_process_plugin_store_error("/a/dprint.json", mark), None);
    // which is forgotten
    assert_eq!(
      session_permissions.take_process_plugin_store_error("/a/dprint.json", ProcessPluginStoreErrorMark(0)),
      None
    );

    // one since then is taken once, and only for its config file
    session_permissions.set_process_plugin_store_error("/a/dprint.json".to_string(), "new".to_string());
    assert_eq!(session_permissions.take_process_plugin_store_error("/b/dprint.json", mark), None);
    assert_eq!(
      session_permissions.take_process_plugin_store_error("/a/dprint.json", mark).as_deref(),
      Some("new")
    );
    assert_eq!(session_permissions.take_process_plugin_store_error("/a/dprint.json", mark), None);

    // and not once storing succeeded since
    session_permissions.set_process_plugin_store_error("/a/dprint.json".to_string(), "new".to_string());
    session_permissions.clear_process_plugin_store_error("/a/dprint.json");
    assert_eq!(session_permissions.take_process_plugin_store_error("/a/dprint.json", mark), None);
  }

  #[test]
  fn should_use_stored_and_session_decisions() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    for decision in [
      decision("/a/dprint.json", "1:a", true),
      decision("/b/dprint.json", "1:a", false),
      decision("/a/dprint.json", "1:b", true),
    ] {
      update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(decision)).unwrap();
    }
    assert_eq!(
      read_editor_approvals(&environment).unwrap().process_plugin_decisions,
      vec![decision("/b/dprint.json", "1:a", false), decision("/a/dprint.json", "1:b", true)]
    );

    let permissions = read_permissions(
      &environment,
      &[decision("/c/dprint.json", "1:a", true), decision("/a/dprint.json", "1:b", false)],
    );
    // the session decision wins
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:b")));
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:b")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:a")));
    assert!(permissions.is_process_plugin_denied(&approval("/b/dprint.json", "1:a")));
    assert!(permissions.is_process_plugin_allowed(&approval("/c/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/d/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_denied(&approval("/d/dprint.json", "1:a")));
    assert!(permissions.was_allowed_with_other_config(&approval("/c/dprint.json", "1:c")));
    // the most recent decision denied it
    assert!(!permissions.was_allowed_with_other_config(&approval("/a/dprint.json", "1:c")));
    assert!(!permissions.was_allowed_with_other_config(&approval("/b/dprint.json", "1:c")));
  }

  #[test]
  fn should_ask_again_when_the_most_recent_decision_is_for_another_config() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let plugin = approval("/a/dprint.json", "1:a").plugin;
    // ex. denied for the session, then allowed by running `dprint fmt`
    let denied = ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
      allowed: false,
      decided_at: 1,
    };
    let allowed = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1:a", true)
    };
    let permissions = read_permissions(&environment, &[denied.clone(), allowed.clone()]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // the configuration changed since, so the older denial isn't used and the user is asked
    let changed = approval("/a/dprint.json", "1:b");
    assert!(!permissions.is_process_plugin_allowed(&changed));
    assert!(!permissions.is_process_plugin_denied(&changed));
    assert!(!permissions.is_process_plugin_always_denied(&changed));
    assert!(permissions.was_allowed_with_other_config(&changed));
    assert!(!permissions.is_process_plugin_denied(&ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone())));

    // denying it after it was allowed denies it with each configuration
    let denied_since = ProcessPluginDecision { decided_at: 3, ..denied };
    let permissions = read_permissions(&environment, &[allowed, denied_since]);
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    assert!(permissions.is_process_plugin_denied(&changed));
    assert!(!permissions.was_allowed_with_other_config(&changed));
  }

  #[test]
  fn should_not_replace_a_more_recent_decision() {
    let mut approvals = EditorApprovals::default();
    let plugin = approval("/a/dprint.json", "1:a").plugin;
    let denied = ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin),
      allowed: false,
      decided_at: 2,
    };
    approvals.add_process_plugin_decision(denied.clone());
    // ex. allowed setting it up before it was denied in another window
    approvals.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 1,
      ..decision("/a/dprint.json", "1:a", true)
    });
    assert_eq!(approvals.process_plugin_decisions, vec![denied.clone()]);
    // a more recent one replaces it
    let allowed = ProcessPluginDecision {
      decided_at: 3,
      ..decision("/a/dprint.json", "1:a", true)
    };
    approvals.add_process_plugin_decision(allowed.clone());
    assert_eq!(approvals.process_plugin_decisions, vec![allowed.clone()]);
    // ex. a plugin allowed before the configuration changed and `dprint fmt` allowed it is set up
    approvals.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1:b", true)
    });
    assert_eq!(approvals.process_plugin_decisions, vec![allowed.clone()]);
    // and a denial that's older doesn't either
    approvals.add_process_plugin_decision(ProcessPluginDecision { decided_at: 2, ..denied });
    assert_eq!(approvals.process_plugin_decisions, vec![allowed]);
    // one that's as recent does
    let allowed = ProcessPluginDecision {
      decided_at: 3,
      ..decision("/a/dprint.json", "1:b", true)
    };
    approvals.add_process_plugin_decision(allowed.clone());
    assert_eq!(approvals.process_plugin_decisions, vec![allowed]);
  }

  #[test]
  fn should_use_the_most_recent_decision() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let stored = |allowed, decided_at| {
      update_editor_approvals_without_lock(&environment, |approvals| {
        approvals.add_process_plugin_decision(ProcessPluginDecision {
          decided_at,
          ..decision("/a/dprint.json", "1:a", allowed)
        })
      })
      .unwrap();
    };
    let session_decision = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1:a", false)
    };

    stored(true, 1);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    // ex. allowing it by running `dprint fmt` after denying it for the session
    stored(true, 3);
    assert!(read_permissions(&environment, std::slice::from_ref(&session_decision)).is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
  }

  #[test]
  fn should_deny_a_process_plugin_with_any_config() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let any_config = |config_file: &str, allowed| ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config(config_file.to_string(), approval(config_file, "1:a").plugin),
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
    let a_approval = ProcessPluginApproval::any_config("/a/dprint.json".to_string(), approval("/a/dprint.json", "1:a").plugin);
    assert!(permissions.is_process_plugin_denied(&a_approval));
    // it's denied with each configuration
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // IMPORTANT: allowing it with any configuration doesn't allow it
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:a")));
    // allowing it with its configuration afterwards wins (ex. running `dprint fmt`)
    let allowed = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1:a", true)
    };
    let permissions = read_permissions(&environment, &[allowed]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:b")));
  }

  #[test]
  fn should_keep_a_decision_for_each_plugin_of_a_config_file() {
    let mut state = EditorApprovals::default();
    let other_plugin = |config_hash: &str, allowed| {
      let mut decision = decision("/a/dprint.json", config_hash, allowed);
      decision.approval.plugin = "https://plugins.dprint.dev/other.json@checksum".to_string();
      decision
    };
    state.add_process_plugin_decision(decision("/a/dprint.json", "1:a", true));
    state.add_process_plugin_decision(other_plugin("1:a", true));
    state.add_process_plugin_decision(other_plugin("1:b", false));
    assert_eq!(
      state.process_plugin_decisions,
      vec![decision("/a/dprint.json", "1:a", true), other_plugin("1:b", false)]
    );
  }

  #[test]
  fn should_allow_setting_up_a_process_plugin_for_the_session() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let plugin = approval("/a/dprint.json", "1:a").plugin;
    assert!(
      !session_permissions
        .read(&environment)
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None)
    );
    session_permissions.add_process_plugin_set_up_grant(ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin: plugin.clone(),
      trusted_config_hash: "hash".to_string(),
      unpinned_file_checksum: None,
      always: true,
      decided_at: 1,
    });
    let permissions = session_permissions.read(&environment);
    assert!(permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None));
    assert!(!permissions.may_set_up_process_plugin("/b/dprint.json", &plugin, "hash", None));
    // it's not allowed once the config file changes
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "other", None));
    // it's not allowed with any configuration
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(session_permissions.take_process_plugin_set_up_grant("/a/dprint.json", &plugin).unwrap().always);
    assert!(
      !session_permissions
        .read(&environment)
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None)
    );
  }

  #[test]
  fn should_not_set_up_a_process_plugin_that_was_denied_since_it_was_allowed() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let plugin = approval("/a/dprint.json", "1:a").plugin;
    let grant = |decided_at| ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin: plugin.clone(),
      trusted_config_hash: "hash".to_string(),
      unpinned_file_checksum: None,
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
        .may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None)
    );
    // denied after (ex. in another window)
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
      allowed: false,
      decided_at: 3,
    });
    let permissions = session_permissions.read(&environment);
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None));
    assert!(permissions.is_denied_since(&grant(2)));
  }

  #[test]
  fn should_let_a_denial_stored_after_a_session_allow_win() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let plugin = "https://plugins.dprint.dev/plugin.json@checksum".to_string();
    // ex. the time of a decision from the future that was forgotten since
    let session_allow = ProcessPluginDecision {
      decided_at: 1000,
      ..decision("/a/dprint.json", "1:a", true)
    };
    assert!(
      !session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
    session_permissions.add_process_plugin_decision(session_allow.clone());
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 1000,
      ..decision("/b/dprint.json", "1:b", true)
    });
    session_permissions.add_process_plugin_set_up_grant(ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin: plugin.clone(),
      trusted_config_hash: "hash".to_string(),
      unpinned_file_checksum: None,
      always: false,
      decided_at: 1000,
    });
    let permissions = session_permissions.read(&environment);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None));

    // ex. "Always Deny" on a machine whose clock is behind
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(ProcessPluginDecision {
        approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
        allowed: false,
        decided_at: 10,
      })
    })
    .unwrap();
    let permissions = session_permissions.read(&environment);
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.may_set_up_process_plugin("/a/dprint.json", &plugin, "hash", None));
    // the decisions for other plugins are kept
    assert!(permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:b")));

    // a denial that was read before is compared by when it was made
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 11,
      ..session_allow
    });
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
  }

  #[test]
  fn should_only_restore_the_allows_of_a_session_when_the_approvals_file_did_not_change() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    environment.set_fs_time(1);
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(decision("/b/dprint.json", "1:b", true))
    })
    .unwrap();
    let session_permissions = SessionPermissions::default();
    session_permissions.read(&environment);
    session_permissions.add_process_plugin_decision(decision("/a/dprint.json", "1:a", true));
    session_permissions.add_process_plugin_decision(decision("/c/dprint.json", "1:c", false));
    session_permissions.add_process_plugin_set_up_grant(ProcessPluginSetUpGrant {
      config_file: "/d/dprint.json".to_string(),
      plugin: "plugin".to_string(),
      trusted_config_hash: "hash".to_string(),
      unpinned_file_checksum: None,
      always: false,
      decided_at: 0,
    });
    let state = session_permissions.state();
    assert!(state.approvals_file_fingerprint.is_some());
    let restore = |state: SessionPermissionsState| {
      let restored = SessionPermissions::default();
      restored.restore(&environment, state);
      restored
    };

    // the file didn't change, so it's like the server never stopped
    let restored = restore(state.clone());
    let permissions = restored.read(&environment);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(permissions.may_set_up_process_plugin("/d/dprint.json", "plugin", "hash", None));

    // ex. an "Always Deny" was stored and forgotten in another window while the server was stopped,
    // which leaves the same text
    let text = environment.read_file(get_editor_approvals_file_path(&environment).unwrap()).unwrap();
    environment.set_fs_time(2);
    environment.write_file(get_editor_approvals_file_path(&environment).unwrap(), &text).unwrap();
    let restored = restore(state.clone());
    let permissions = restored.read(&environment);
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.may_set_up_process_plugin("/d/dprint.json", "plugin", "hash", None));
    // but the denials are kept
    assert!(permissions.is_process_plugin_denied(&approval("/c/dprint.json", "1:c")));
    // and the stored allows are still used
    assert!(permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:b")));
    // which is what the session file keeps from now on
    assert_eq!(restored.state().approvals_file_fingerprint, read_editor_approvals_fingerprint(&environment));

    // and so is a session whose fingerprint isn't known (ex. reading the file failed)
    let restored = restore(SessionPermissionsState {
      approvals_file_fingerprint: None,
      ..session_permissions.state()
    });
    assert!(!restored.read(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
  }

  #[test]
  fn should_not_write_the_approvals_file_without_its_lock() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    environment.clone().run_in_runtime(async move {
      let lock_path = get_editor_approvals_lock_path(&environment);
      // ex. another process holds it
      let other_lock = environment.try_lock_file(&lock_path).unwrap().unwrap();
      let err = lock_editor_approvals_within(&environment, Duration::from_millis(50)).await.unwrap_err();
      assert!(err.to_string().starts_with("Timed out waiting for another dprint process"), "{}", err);
      drop(other_lock);
      assert!(lock_editor_approvals_within(&environment, Duration::from_millis(50)).await.is_ok());

      // ex. the file system doesn't support locks
      environment.set_file_lock_fails(&lock_path, true);
      let err = store_process_plugin_decision(&environment, decision("/a/dprint.json", "1:a", true))
        .await
        .unwrap_err();
      assert!(format!("{:#}", err).starts_with("Failed locking"), "{:#}", err);
      assert!(!environment.path_exists(get_editor_approvals_file_path(&environment).unwrap()));
      environment.set_file_lock_fails(&lock_path, false);
      store_process_plugin_decision(&environment, decision("/a/dprint.json", "1:a", true))
        .await
        .unwrap();
      environment.set_file_lock_fails(&lock_path, true);
      assert!(forget_editor_approvals(&environment, |_| true).await.is_err());
      assert_eq!(read_editor_approvals(&environment).unwrap().process_plugin_decisions.len(), 1);
      // but forgetting what isn't there doesn't need it
      forget_editor_approvals(&environment, |config_file| config_file == "/b/dprint.json")
        .await
        .unwrap();
    });
  }

  #[test]
  fn should_keep_the_stored_denials_the_session_saw_when_its_state_is_restored() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let plugin = "https://plugins.dprint.dev/plugin.json@checksum".to_string();
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(ProcessPluginDecision {
        approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
        allowed: false,
        decided_at: 10,
      })
    })
    .unwrap();
    let session_permissions = SessionPermissions::default();
    session_permissions.read(&environment);
    // allowed for the session after the denial was seen
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 11,
      ..decision("/a/dprint.json", "1:a", true)
    });
    let state = session_permissions.state();
    assert_eq!(state.seen_stored_denials.len(), 1);

    // ex. a server that was started again for the session
    let restored = SessionPermissions::default();
    restored.restore(&environment, state.clone());
    assert!(restored.read(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert_eq!(restored.state(), state);
    // the denial would otherwise look like it was stored since
    let restored = SessionPermissions::default();
    restored.restore(
      &environment,
      SessionPermissionsState {
        seen_stored_denials: Vec::new(),
        ..state
      },
    );
    assert!(!restored.read(&environment).is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
  }

  #[test]
  fn should_record_when_the_decision_was_made() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(5);
    assert_eq!(
      ProcessPluginDecision::new(&environment, approval("/a/dprint.json", "1:a"), true, &[]).decided_at,
      5000
    );
    // it's after the decisions for the plugin, even one that seems to be from the future (ex. from a machine whose clock is ahead)
    let decisions = [
      ProcessPluginDecision {
        decided_at: 9000,
        ..decision("/a/dprint.json", "1:a", false)
      },
      ProcessPluginDecision {
        decided_at: 12000,
        ..decision("/b/dprint.json", "1:a", false)
      },
    ];
    assert_eq!(
      ProcessPluginDecision::new(&environment, approval("/a/dprint.json", "1:b"), true, &decisions).decided_at,
      9001
    );
    let grant = ProcessPluginSetUpGrant::new(
      &environment,
      "/a/dprint.json".to_string(),
      approval("/a/dprint.json", "1:a").plugin,
      "hash".to_string(),
      None,
      false,
      &decisions,
    );
    assert_eq!(grant.decided_at, 9001);
    assert_eq!(
      ProcessPluginDecision::new(&environment, approval("/c/dprint.json", "1:a"), true, &decisions).decided_at,
      5000
    );
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
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
    session_permissions.read(&environment);
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        format!("Unknown property in {}: editor.allowProcessPlugin", file_path.display()),
        format!("Unknown property in {}: other", file_path.display()),
        format!(
          "Requiring approval to run process plugins and to download from any domain in editors, and not using the stored \"Always Allow\" decisions, because {} has a property in \"editor\" that this version of dprint doesn't know (editor.allowProcessPlugin).",
          file_path.display()
        ),
      ]
    );
  }

  #[test]
  fn should_not_allow_process_plugins_with_an_unknown_editor_setting() {
    let environment = TestEnvironment::new();
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    // ex. a setting of a newer version of dprint that narrows what's allowed
    environment
      .write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": true, "processPluginExceptions": [] } }"#)
      .unwrap();
    let permissions = read_permissions(&environment, &[]);
    assert_eq!(permissions.allow_process_plugins, Some(false));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // the messages that ask the user say why, because it's what the user would otherwise do
    let message = format!(
      concat!(
        "dprint {} doesn't use its settings file in editors because {} has a property in \"editor\" that this version of dprint ",
        "doesn't know (editor.processPluginExceptions). Update dprint or change the file to use it."
      ),
      environment.cli_version(),
      file_path.display()
    );
    assert_eq!(permissions.ignored_settings_message.as_deref(), Some(message.as_str()));
    assert_eq!(read_ignored_settings_message(&environment), Some(message));
    // what was decided for the session is still used
    let permissions = read_permissions(&environment, &[decision("/a/dprint.json", "1:a", true)]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // an unknown property outside of "editor" is only logged
    environment
      .write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": true }, "other": 1 }"#)
      .unwrap();
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert_eq!(permissions.ignored_settings_message, None);
    assert_eq!(read_ignored_settings_message(&environment), None);
    environment.take_stderr_messages();
  }

  #[test]
  fn should_only_use_the_stored_denials_when_the_settings_are_not_used() {
    let environment = TestEnvironment::new();
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment.write_file(&file_path, r#"{ "editor": {} }"#).unwrap();
    let allowed = approval("/a/dprint.json", "1:a");
    let denied = approval("/b/dprint.json", "1:b");
    let replaced = approval("/c/dprint.json", "1:c");
    for decision in [
      decision("/a/dprint.json", "1:a", true),
      decision("/b/dprint.json", "1:b", false),
      ProcessPluginDecision {
        approval: ProcessPluginApproval::any_config(replaced.config_file.clone(), replaced.plugin.clone()),
        allowed: false,
        decided_at: 0,
      },
      ProcessPluginDecision {
        approval: replaced.clone(),
        allowed: true,
        decided_at: 1,
      },
    ] {
      update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(decision)).unwrap();
    }
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_allowed(&allowed));
    assert!(permissions.is_process_plugin_allowed(&replaced));
    assert!(permissions.is_process_plugin_always_denied(&denied));
    // ex. a setting of a newer version of dprint that never allows process plugins
    environment
      .write_file(&file_path, r#"{ "editor": { "allowProcessPlugins": "never" } }"#)
      .unwrap();
    let permissions = read_permissions(&environment, &[]);
    assert_eq!(permissions.allow_process_plugins, Some(false));
    assert!(!permissions.is_process_plugin_allowed(&allowed));
    assert!(!permissions.is_process_plugin_denied(&allowed));
    assert_eq!(permissions.get_latest_allow(&allowed.config_file, &allowed.plugin), None);
    assert!(permissions.is_process_plugin_always_denied(&denied));
    // and a denial that an allow replaced isn't used either
    assert!(!permissions.is_process_plugin_allowed(&replaced));
    assert!(!permissions.is_process_plugin_denied(&replaced));
    environment.take_stderr_messages();
  }

  #[test]
  fn should_not_allow_process_plugins_with_a_trusted_domain_this_version_does_not_know() {
    let environment = TestEnvironment::new();
    let file_path = get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    // ex. a value of a newer version of dprint that trusts less
    environment
      .write_file(
        &file_path,
        r#"{ "editor": { "allowProcessPlugins": true, "trustedDomains": ["!github.com"] } }"#,
      )
      .unwrap();
    let permissions = read_permissions(&environment, &[]);
    assert_eq!(permissions.allow_process_plugins, Some(false));
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    let reason = format!(
      "{} has a value in \"editor.trustedDomains\" that isn't a domain (\"!github.com\")",
      file_path.display()
    );
    assert!(permissions.ignored_settings_message.unwrap().contains(&reason));
    assert_eq!(
      environment.take_stderr_messages(),
      vec![format!(
        "Requiring approval to run process plugins and to download from any domain in editors, and not using the stored \"Always Allow\" decisions, because {}.",
        reason
      )]
    );
  }

  #[test]
  fn should_forget_the_oldest_decisions() {
    let mut approvals = EditorApprovals::default();
    approvals.add_process_plugin_decision(decision("/denied/dprint.json", "1:a", false));
    for i in 0..MAX_PROCESS_PLUGIN_DECISIONS + 2 {
      approvals.add_process_plugin_decision(decision(&format!("/{i}/dprint.json"), "1:a", true));
    }
    assert_eq!(approvals.process_plugin_decisions.len(), MAX_PROCESS_PLUGIN_DECISIONS);
    // what was denied is kept
    assert_eq!(approvals.process_plugin_decisions[0], decision("/denied/dprint.json", "1:a", false));
    assert_eq!(approvals.process_plugin_decisions[1], decision("/3/dprint.json", "1:a", true));
  }

  #[test]
  fn should_not_write_over_approvals_that_do_not_parse() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint with another format
    environment.write_file(&file_path, r#"{ "version": 2, "other": [] }"#).unwrap();
    let session_permissions = SessionPermissions::default();
    session_permissions.add_process_plugin_decision(decision("/a/dprint.json", "1:a", true));
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
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
        "1:a",
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
  fn should_keep_the_additions_of_a_newer_version_when_writing_approvals() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint added a property of the file and of a decision, a kind of
    // decision, and another way of making the config hash, which don't raise `minVersion`
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
      "configHash": "2:b",
      "allowed": true,
      "decidedAt": 0
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
      vec![
        decision("/a/dprint.json", "1:a", false),
        decision("/b/dprint.json", "2:b", true),
        decision("/b/dprint.json", "1:b", true)
      ]
    );
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
    // an allow with a config hash made another way is ignored
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "2:b")));
    assert!(permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:b")));
    assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());

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
      "configHash": "2:b",
      "allowed": true,
      "decidedAt": 0
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
    // ex. a newer version of dprint narrowed what an allow covers
    let text = r#"{
  "version": 2,
  "minVersion": 2,
  "processPluginDecisions": [
    { "configFile": "/a/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:a", "allowed": true, "decidedAt": 0, "workspace": "/a" },
    { "configFile": "/b/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:b", "allowed": false, "decidedAt": 0 }
  ]
}"#;
    environment.write_file(&file_path, text).unwrap();
    let err = read_editor_approvals(&environment).unwrap_err();
    assert_eq!(
      err.to_string(),
      format!("{} is from a newer version of dprint, so it can't be read.", file_path.display())
    );
    assert_eq!(err.downcast_ref::<NewerApprovalsFileError>().unwrap().file_path, file_path);
    // IMPORTANT: An allow isn't used because it may allow too much.
    let permissions = read_permissions(&environment, &[]);
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // but what was denied is still denied, because denying more than the user did is safe
    assert!(permissions.is_process_plugin_denied(&approval("/b/dprint.json", "1:b")));
    assert!(permissions.is_process_plugin_always_denied(&approval("/b/dprint.json", "1:b")));
    assert_eq!(read_editor_approval_denials(&environment), vec![decision("/b/dprint.json", "1:b", false)]);
    assert_eq!(environment.take_stderr_messages().len(), 1);
  }

  #[test]
  fn should_not_write_to_approvals_with_a_newer_min_version() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint changed the structure of the file
    let text = r#"{
  "version": 3,
  "minVersion": 3,
  "other": { "a": 1 },
  "processPluginDecisions": [
    { "configFile": "/a/dprint.json", "kind": "other", "allowed": true },
    { "configFile": "/b/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:b", "allowed": true, "decidedAt": 0 },
    { "configFile": "/c/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "allowed": false, "decidedAt": 0 }
  ],
  "otherDecisions": [{ "configFile": "/a/dprint.json" }]
}
"#;
    environment.write_file(&file_path, text).unwrap();
    let assert_newer_error = |result: Result<()>| {
      let err = result.unwrap_err();
      assert_eq!(err.downcast_ref::<NewerApprovalsFileError>().unwrap().file_path, file_path);
    };
    // neither an allow nor a denial is stored
    assert_newer_error(update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(decision("/d/dprint.json", "1:d", true))
    }));
    assert_newer_error(update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(ProcessPluginDecision {
        approval: ProcessPluginApproval::any_config("/b/dprint.json".to_string(), "https://plugins.dprint.dev/plugin.json@checksum".to_string()),
        allowed: false,
        decided_at: 1,
      })
    }));
    assert_eq!(environment.read_file(&file_path).unwrap(), text);

    environment.clone().run_in_runtime({
      let environment = environment.clone();
      let file_path = file_path.clone();
      async move {
        // the decisions of a config file aren't forgotten
        let err = forget_editor_approvals(&environment, |config_file| config_file == "/a/dprint.json")
          .await
          .unwrap_err();
        assert_eq!(err.downcast_ref::<NewerApprovalsFileError>().unwrap().file_path, file_path);
        let err = forget_editor_approvals(&environment, |_| true).await.unwrap_err();
        assert!(err.downcast_ref::<NewerApprovalsFileError>().is_some());
        // which doesn't fail when there are none
        forget_editor_approvals(&environment, |config_file| config_file == "/e/dprint.json")
          .await
          .unwrap();
        assert_eq!(environment.read_file(&file_path).unwrap(), text);
      }
    });
    // the denials are still applied
    let permissions = read_permissions(&environment, &[]);
    assert!(permissions.is_process_plugin_always_denied(&approval("/c/dprint.json", "1:c")));
    assert!(!permissions.is_process_plugin_allowed(&approval("/b/dprint.json", "1:b")));
    assert_eq!(environment.take_stderr_messages().len(), 1);
  }

  #[test]
  fn should_treat_versions_that_are_not_integers_as_newer() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    for (version, min_version) in [
      (r#""3""#, r#""3""#),
      ("3.0", "3.0"),
      ("1", "99999999999999999999"),
      ("1", "-1"),
      (r#""3""#, "null"),
    ] {
      let text = format!(
        r#"{{
  "version": {},
  "minVersion": {},
  "processPluginDecisions": [
    {{ "configFile": "/a/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "1:a", "allowed": true, "decidedAt": 0 }},
    {{ "configFile": "/b/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "allowed": false, "decidedAt": 0 }}
  ]
}}
"#,
        version, min_version
      );
      environment.write_file(&file_path, &text).unwrap();
      assert!(read_editor_approvals(&environment).is_err(), "{}", text);
      let permissions = read_permissions(&environment, &[]);
      assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")), "{}", text);
      // the denials are still applied
      assert!(permissions.is_process_plugin_denied(&approval("/b/dprint.json", "1:b")), "{}", text);
      environment.take_stderr_messages();
    }

    // a version that isn't an integer is kept as it is when the file is written
    environment
      .write_file(&file_path, r#"{ "version": "2.1", "minVersion": 1, "processPluginDecisions": [] }"#)
      .unwrap();
    update_editor_approvals_without_lock(&environment, |_| {}).unwrap();
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      "{ \"version\": \"2.1\", \"minVersion\": 1, \"processPluginDecisions\": [] }\n"
    );
  }

  #[test]
  fn should_not_treat_a_stored_denial_that_was_read_as_new_after_a_failed_read() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let session_permissions = SessionPermissions::default();
    let denial = ProcessPluginDecision {
      decided_at: 10,
      ..decision("/a/dprint.json", "1:a", false)
    };
    update_editor_approvals_without_lock(&environment, |approvals| approvals.add_process_plugin_decision(denial.clone())).unwrap();
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_denied(&approval("/a/dprint.json", "1:a"))
    );
    // the user allows it for the session, which is more recent
    session_permissions.add_process_plugin_decision(ProcessPluginDecision {
      decided_at: 11,
      ..decision("/a/dprint.json", "1:a", true)
    });
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
    // a read fails (ex. the user saved the file with a typo)
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    let text = environment.read_file(&file_path).unwrap();
    environment.write_file(&file_path, "{ nope").unwrap();
    session_permissions.read(&environment);
    assert_eq!(environment.take_stderr_messages().len(), 1);
    // which doesn't make the denial look like it was stored since
    environment.write_file(&file_path, &text).unwrap();
    assert!(
      session_permissions
        .read(&environment)
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
  }

  #[test]
  fn should_not_apply_a_denial_that_only_an_out_of_date_read_saw() {
    let session_permissions = SessionPermissions::default();
    let session_allow = ProcessPluginDecision {
      decided_at: 11,
      ..decision("/a/dprint.json", "1:a", true)
    };
    session_permissions.add_process_plugin_decision(session_allow.clone());
    let denial = ProcessPluginDecision {
      decided_at: 10,
      ..decision("/a/dprint.json", "1:a", false)
    };
    // a read that started first finishes after one that started later, which saw that the denial was forgotten
    let out_of_date_read = session_permissions.start_stored_decisions_read();
    let read = session_permissions.start_stored_decisions_read();
    session_permissions.remove_allows_denied_since(read, &[], None);
    session_permissions.remove_allows_denied_since(out_of_date_read, std::slice::from_ref(&denial), None);
    assert_eq!(session_permissions.session_process_plugin_decisions(), vec![session_allow.clone()]);
    // but a denial that's stored again is another one
    let read = session_permissions.start_stored_decisions_read();
    let stored_again = ProcessPluginDecision { decided_at: 12, ..denial };
    session_permissions.remove_allows_denied_since(read, &[stored_again], None);
    assert_eq!(session_permissions.session_process_plugin_decisions(), Vec::new());
  }

  #[test]
  fn should_forget_the_decisions_of_other_kinds() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint stores decisions of another kind
    environment
      .write_file(
        &file_path,
        r#"{
  "version": 3,
  "minVersion": 1,
  "other": { "configFile": "/a/dprint.json" },
  "processPluginDecisions": [],
  "downloadDecisions": [
    { "configFile": "/a/dprint.json", "allowed": true },
    { "configFile": "/b/dprint.json", "allowed": true },
    { "allowed": true }
  ],
  "otherDecisions": { "allowed": true }
}
"#,
      )
      .unwrap();
    environment.clone().run_in_runtime(async move {
      // writing a decision keeps them
      update_editor_approvals_without_lock(&environment, |approvals| {
        approvals.add_process_plugin_decision(decision("/a/dprint.json", "1:a", true))
      })
      .unwrap();
      let text = environment.read_file(&file_path).unwrap();
      assert_eq!(text.matches(r#""allowed": true"#).count(), 5, "{}", text);

      // the ones of the config file
      forget_editor_approvals(&environment, |config_file| config_file == "/a/dprint.json")
        .await
        .unwrap();
      let text = environment.read_file(&file_path).unwrap();
      assert_eq!(
        text,
        r#"{
  "version": 3,
  "minVersion": 1,
  "other": { "configFile": "/a/dprint.json" },
  "processPluginDecisions": [],
  "downloadDecisions": [
    { "configFile": "/b/dprint.json", "allowed": true },
    { "allowed": true }
  ],
  "otherDecisions": { "allowed": true }
}
"#
      );

      // and every one, which keeps the properties that don't have decisions
      forget_editor_approvals(&environment, |_| true).await.unwrap();
      let text = environment.read_file(&file_path).unwrap();
      assert_eq!(
        text,
        r#"{
  "version": 3,
  "minVersion": 1,
  "other": { "configFile": "/a/dprint.json" },
  "processPluginDecisions": [],
  "downloadDecisions": []
}
"#
      );
    });
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
  fn should_not_let_an_allow_with_a_hash_made_another_way_hide_one_this_version_makes() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let at = |decision: ProcessPluginDecision, decided_at| ProcessPluginDecision { decided_at, ..decision };
    // ex. a newer version of dprint that hashes the configuration another way allowed it since
    let permissions = read_permissions(
      &environment,
      &[at(decision("/a/dprint.json", "1:a", true), 1), at(decision("/a/dprint.json", "2:a", true), 2)],
    );
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(!permissions.was_allowed_with_other_config(&approval("/a/dprint.json", "1:a")));
    // and a hash without a version isn't one this version makes
    let permissions = read_permissions(
      &environment,
      &[at(decision("/a/dprint.json", "1:a", true), 1), at(decision("/a/dprint.json", "a", true), 2)],
    );
    assert!(permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    // a denial with any hash is still applied
    let permissions = read_permissions(
      &environment,
      &[at(decision("/a/dprint.json", "1:a", true), 1), at(decision("/a/dprint.json", "2:a", false), 2)],
    );
    assert!(!permissions.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
  }

  #[test]
  fn should_keep_an_allow_with_a_hash_made_another_way_when_storing_an_allow() {
    let at = |decision: ProcessPluginDecision, decided_at| ProcessPluginDecision { decided_at, ..decision };
    let other_way = at(decision("/a/dprint.json", "2:a", true), 2);
    let mut approvals = EditorApprovals::default();
    approvals.add_process_plugin_decision(at(decision("/a/dprint.json", "1:a", true), 1));
    approvals.add_process_plugin_decision(other_way.clone());
    // ex. the other version stored an allow since, which doesn't keep this version from storing its own
    assert!(approvals.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert_eq!(
      approvals.process_plugin_decisions,
      vec![at(decision("/a/dprint.json", "1:a", true), 1), other_way.clone()]
    );
    // an allow replaces the one made the same way and keeps the other
    approvals.add_process_plugin_decision(at(decision("/a/dprint.json", "1:b", true), 1));
    assert_eq!(
      approvals.process_plugin_decisions,
      vec![other_way.clone(), at(decision("/a/dprint.json", "1:b", true), 1)]
    );
    assert!(!approvals.is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(approvals.is_process_plugin_allowed(&approval("/a/dprint.json", "1:b")));
    // a denial that's older doesn't replace the allow made the other way
    let denied = ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), other_way.approval.plugin.clone()),
      allowed: false,
      decided_at: 1,
    };
    approvals.add_process_plugin_decision(denied.clone());
    assert_eq!(
      approvals.process_plugin_decisions,
      vec![other_way, at(decision("/a/dprint.json", "1:b", true), 1)]
    );
    // but a more recent one replaces every allow
    let denied = ProcessPluginDecision { decided_at: 3, ..denied };
    approvals.add_process_plugin_decision(denied.clone());
    assert_eq!(approvals.process_plugin_decisions, vec![denied]);
  }

  #[test]
  fn should_keep_the_allows_of_each_version_when_two_versions_use_the_approvals_file() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let file_path = get_editor_approvals_file_path(&environment).unwrap();
    // ex. a newer version of dprint that hashes the configuration another way stored an allow
    environment
      .write_file(
        &file_path,
        r#"{
  "version": 1,
  "processPluginDecisions": [
    { "configFile": "/a/dprint.json", "plugin": "https://plugins.dprint.dev/plugin.json@checksum", "configHash": "2:a", "allowed": true, "decidedAt": 5 }
  ]
}
"#,
      )
      .unwrap();
    update_editor_approvals_without_lock(&environment, |approvals| {
      approvals.add_process_plugin_decision(ProcessPluginDecision {
        decided_at: 1,
        ..decision("/a/dprint.json", "1:a", true)
      })
    })
    .unwrap();
    let text = environment.read_file(&file_path).unwrap();
    assert!(text.contains(r#""configHash": "2:a""#), "{}", text);
    assert!(text.contains(r#""configHash": "1:a""#), "{}", text);
    assert!(read_permissions(&environment, &[]).is_process_plugin_allowed(&approval("/a/dprint.json", "1:a")));
    assert!(
      read_editor_approvals(&environment)
        .unwrap()
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
  }

  #[test]
  fn should_let_a_denial_win_a_tie() {
    let environment = TestEnvironment::new();
    require_approval(&environment);
    let plugin = approval("/a/dprint.json", "1:a").plugin;
    // ex. two windows decided at the same time after a decision from the future
    let denied = ProcessPluginDecision {
      approval: ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin.clone()),
      allowed: false,
      decided_at: 2,
    };
    let allowed = ProcessPluginDecision {
      decided_at: 2,
      ..decision("/a/dprint.json", "1:a", true)
    };
    // whichever is last
    for decisions in [[denied.clone(), allowed.clone()], [allowed.clone(), denied.clone()]] {
      let permissions = read_permissions(&environment, &decisions);
      assert!(permissions.is_process_plugin_denied(&approval("/a/dprint.json", "1:a")));
      let mut approvals = EditorApprovals::default();
      for decision in decisions {
        approvals.add_process_plugin_decision(decision);
      }
      assert_eq!(approvals.process_plugin_decisions, vec![denied.clone()]);
    }
    let grant = ProcessPluginSetUpGrant {
      config_file: "/a/dprint.json".to_string(),
      plugin,
      trusted_config_hash: "hash".to_string(),
      unpinned_file_checksum: None,
      always: true,
      decided_at: 2,
    };
    assert!(read_permissions(&environment, std::slice::from_ref(&denied)).is_denied_since(&grant));
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
        .is_process_plugin_allowed(&approval("/a/dprint.json", "1:a"))
    );
    // only logged once
    session_permissions.read(&environment);
    let messages = environment.take_stderr_messages();
    assert_eq!(messages.len(), 1);
    assert!(
      messages[0]
        .starts_with("Requiring approval to run process plugins and to download from any domain in editors, and not using the stored \"Always Allow\" decisions, because the settings couldn't be read: "),
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
