//! The file that keeps what the user decided for an editor extension's
//! session, so that a server that's started again for the same session (ex.
//! after it exited unexpectedly) behaves as if it never stopped.

use std::cell::Cell;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;

use super::messages::ApprovalItem;
use super::messages::ConfigFileIdentifier;
use crate::configuration::SessionPermissionsState;
use crate::environment::DirEntry;
use crate::environment::Environment;

/// How long a session file is kept after it was last written, which is only
/// for the ones a server didn't delete (ex. it crashed and wasn't started
/// again). A server keeps its own file newer than this (see
/// `SESSION_FILE_REFRESH_INTERVAL`).
pub const STALE_SESSION_FILE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How often a server writes its session file again so that it isn't stale
/// while the session lasts.
pub const SESSION_FILE_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The directory in the cache directory with the session files.
const SESSIONS_DIR_NAME: &str = "editor-sessions";
/// The version of the format of a session file, where a file of another
/// version is ignored, which only means the user is asked again.
const SESSION_FILE_VERSION: u64 = 1;

/// What the user decided for a session, which is what's in its file.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
  #[serde(flatten)]
  pub permissions: SessionPermissionsState,
  /// The downloads the user allowed for the session, with the items they were
  /// shown when known.
  pub allowed_downloads: Vec<AllowedDownload>,
  /// The downloads the user denied for the session.
  pub denied_downloads: Vec<DeniedDownload>,
}

/// A download the user allowed for the session (see `DownloadPolicy::allow`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedDownload {
  pub download: String,
  /// What the user was shown, which is what the decisions show.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub item: Option<ApprovalItem>,
}

/// Downloads the user denied for a config file this session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeniedDownload {
  /// The id of the approval that was denied.
  pub id: String,
  /// The key of the config file (see `get_config_key`).
  pub config_key: String,
  /// The config file the user was shown.
  pub config_file: ConfigFileIdentifier,
  /// What the user was shown as the approval's items.
  pub items: Vec<ApprovalItem>,
}

/// The session file of a session id.
///
/// It's only for that session, so two servers of different sessions never
/// write to the same file. Two servers of the same session is a mistake of
/// the client, and the last one to write wins.
pub struct SessionFile {
  file_path: PathBuf,
  /// What was last written or read, so the same state isn't written again.
  last_text: RefCell<Option<String>>,
  /// Whether the session ended, which deleted the file, so it's never written
  /// again (ex. by a request that was still running).
  is_closed: Cell<bool>,
}

impl SessionFile {
  /// Gets the session file of a session id, which is `None` when the id
  /// isn't valid (see `is_valid_session_id`).
  pub fn new(environment: &impl Environment, session_id: &str) -> Option<Self> {
    // IMPORTANT: The id is part of a file path, so only use one that can't name another file.
    if !is_valid_session_id(session_id) {
      return None;
    }
    Some(Self {
      file_path: get_sessions_dir(environment).join(format!("{}.json", session_id)),
      last_text: Default::default(),
      is_closed: Default::default(),
    })
  }

  /// Reads what the user decided for the session, which is `None` when the
  /// file doesn't exist or can't be read (ex. it's from a version of dprint
  /// with another format), so the user is asked again.
  pub fn read(&self, environment: &impl Environment) -> Option<SessionState> {
    let text = match environment.maybe_read_file(&self.file_path) {
      Ok(text) => text?,
      Err(err) => {
        log_warn!(environment, "Failed reading {}: {:#}", self.file_path.display(), err);
        return None;
      }
    };
    match parse_session_file(&text) {
      Ok(state) => {
        *self.last_text.borrow_mut() = Some(text);
        Some(state)
      }
      Err(message) => {
        log_warn!(
          environment,
          "Ignoring what was decided for the session in {} because {}.",
          self.file_path.display(),
          message
        );
        None
      }
    }
  }

  /// Writes what the user decided for the session when it changed since it
  /// was last written, or when `force` is `true` (ex. so that it isn't stale).
  /// A failure is only logged because it only means the user may be asked
  /// again.
  pub fn write(&self, environment: &impl Environment, state: &SessionState, force: bool) {
    if self.is_closed.get() {
      return;
    }
    let text = match serde_json::to_string(&SessionFileContent {
      version: SESSION_FILE_VERSION,
      state,
    }) {
      Ok(text) => text,
      Err(err) => {
        log_warn!(environment, "Failed serializing what was decided for the session: {:#}", err);
        return;
      }
    };
    if !force && self.last_text.borrow().as_deref() == Some(text.as_str()) {
      return;
    }
    let result = match self.file_path.parent() {
      Some(dir_path) => environment.mk_dir_all(dir_path),
      None => Ok(()),
    }
    .and_then(|()| environment.atomic_write_file_bytes(&self.file_path, text.as_bytes()));
    match result {
      Ok(()) => *self.last_text.borrow_mut() = Some(text),
      Err(err) => log_warn!(environment, "Failed writing {}: {:#}", self.file_path.display(), err),
    }
  }

  /// Deletes the file once the session ended (ex. the client stopped the
  /// server), which is never written again after.
  pub fn delete(&self, environment: &impl Environment) {
    self.is_closed.set(true);
    if let Err(err) = environment.remove_file(&self.file_path)
      && err.kind() != std::io::ErrorKind::NotFound
    {
      log_debug!(environment, "Failed deleting {}: {:#}", self.file_path.display(), err);
    }
  }
}

/// Gets whether a session id can be used, which is 16 to 128 ASCII letters,
/// digits, underscores, and hyphens (ex. a UUID).
pub fn is_valid_session_id(session_id: &str) -> bool {
  (16..=128).contains(&session_id.len()) && session_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Deletes the session files that weren't written for a while, which are of
/// the sessions whose servers didn't delete them (ex. they crashed and weren't
/// started again).
pub fn remove_stale_session_files(environment: &impl Environment) {
  let dir_path = get_sessions_dir(environment);
  let Ok(entries) = environment.dir_info(&dir_path) else {
    return;
  };
  let now = environment.sys_time_now();
  for entry in entries {
    let DirEntry::File { path } = entry else {
      continue;
    };
    let is_stale = environment
      .fs_metadata(&path)
      .and_then(|metadata| metadata.modified())
      .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age > STALE_SESSION_FILE_AGE));
    if is_stale && let Err(err) = environment.remove_file(&path) {
      log_debug!(environment, "Failed deleting {}: {:#}", path.display(), err);
    }
  }
}

/// What's written to a session file.
#[derive(Serialize)]
struct SessionFileContent<'a> {
  version: u64,
  #[serde(flatten)]
  state: &'a SessionState,
}

/// Parses a session file, which fails with why it can't be used.
fn parse_session_file(text: &str) -> Result<SessionState, String> {
  let value = serde_json::from_str::<serde_json::Value>(text).map_err(|err| format!("it doesn't parse ({})", err))?;
  let version = value.get("version").and_then(|version| version.as_u64());
  if version != Some(SESSION_FILE_VERSION) {
    return Err("it's from a version of dprint with another format".to_string());
  }
  serde_json::from_value::<SessionState>(value).map_err(|err| format!("it can't be read ({})", err))
}

fn get_sessions_dir(environment: &impl Environment) -> PathBuf {
  environment.get_cache_dir().join(SESSIONS_DIR_NAME)
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  const SESSION_ID: &str = "0123456789abcdef";

  #[test]
  fn should_only_use_a_valid_session_id() {
    assert!(is_valid_session_id(SESSION_ID));
    assert!(is_valid_session_id("8b0b1d2e-6d5c-4f1e-9a7b-3c2d1e0f9a8b"));
    assert!(is_valid_session_id(&"a_".repeat(64)));
    // too short or long
    assert!(!is_valid_session_id("0123456789abcde"));
    assert!(!is_valid_session_id(&"a".repeat(129)));
    // what could name another file
    assert!(!is_valid_session_id("../0123456789abcdef"));
    assert!(!is_valid_session_id("0123456789abcdef/x"));
    assert!(!is_valid_session_id("0123456789abcdef.json"));
    assert!(!is_valid_session_id("0123456789abcdé"));
    let environment = TestEnvironment::new();
    assert!(SessionFile::new(&environment, "..\\..\\0123456789").is_none());
  }

  #[test]
  fn should_read_what_was_written() {
    let environment = TestEnvironment::new();
    let file = SessionFile::new(&environment, SESSION_ID).unwrap();
    assert_eq!(file.read(&environment), None);
    let state = SessionState {
      allowed_downloads: vec![AllowedDownload {
        download: "https://example.com/plugin.wasm".to_string(),
        item: None,
      }],
      ..Default::default()
    };
    file.write(&environment, &state, false);
    let file = SessionFile::new(&environment, SESSION_ID).unwrap();
    assert_eq!(file.read(&environment), Some(state));
  }

  #[test]
  fn should_ignore_a_session_file_with_another_format() {
    let environment = TestEnvironment::new();
    let file = SessionFile::new(&environment, SESSION_ID).unwrap();
    environment.mk_dir_all(file.file_path.parent().unwrap()).unwrap();
    for text in [
      r#"{ "version": 2, "allowedDownloads": [], "deniedDownloads": [] }"#,
      r#"{ "allowedDownloads": [], "deniedDownloads": [] }"#,
      r#"{ "version": 1, "allowedDownloads": 1 }"#,
      "not json",
    ] {
      environment.write_file(&file.file_path, text).unwrap();
      assert_eq!(file.read(&environment), None, "{}", text);
      assert_eq!(environment.take_stderr_messages().len(), 1);
    }
  }

  #[test]
  fn should_never_write_the_session_file_once_it_is_deleted() {
    let environment = TestEnvironment::new();
    let file = SessionFile::new(&environment, SESSION_ID).unwrap();
    file.write(&environment, &SessionState::default(), false);
    assert!(environment.path_exists(&file.file_path));
    file.delete(&environment);
    assert!(!environment.path_exists(&file.file_path));
    // ex. a request that was still running
    file.write(&environment, &SessionState::default(), true);
    assert!(!environment.path_exists(&file.file_path));
  }

  #[test]
  fn should_remove_stale_session_files() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let stale = SessionFile::new(&environment, "stale-0123456789").unwrap();
    stale.write(&environment, &SessionState::default(), false);
    environment.set_fs_time(1_000_000 + STALE_SESSION_FILE_AGE.as_secs());
    let recent = SessionFile::new(&environment, "recent-0123456789").unwrap();
    recent.write(&environment, &SessionState::default(), false);
    // it's only stale once it's older than the age
    remove_stale_session_files(&environment);
    assert!(environment.path_exists(&stale.file_path));
    environment.set_fs_time(1_000_001 + STALE_SESSION_FILE_AGE.as_secs());
    remove_stale_session_files(&environment);
    assert!(!environment.path_exists(&stale.file_path));
    assert!(environment.path_exists(&recent.file_path));
  }
}
