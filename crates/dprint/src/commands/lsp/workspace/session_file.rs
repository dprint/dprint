//! The file that keeps what the user decided for an editor extension's
//! session, so that a server that's started again for the same session (ex.
//! after it exited unexpectedly) behaves as if it never stopped.

use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;

use super::messages::ApprovalItem;
use super::messages::ConfigFileIdentifier;
use crate::configuration::SessionPermissionsState;
use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::environment::FileLock;
use crate::environment::PathKind;

/// How long a session file is kept after it was last written by the wall
/// clock, which is only for the ones a server didn't delete (ex. it crashed
/// and wasn't started again). A server keeps its own file newer than this
/// (see `SESSION_FILE_REFRESH_INTERVAL`).
///
/// IMPORTANT: Never lower this, because every version of dprint deletes the
/// files of the others by it (see `SessionFile`).
pub const STALE_SESSION_FILE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How long after a server last wrote its session file by the wall clock it
/// writes it again so that it isn't stale while the session lasts.
///
/// IMPORTANT: Keep this well below `STALE_SESSION_FILE_AGE` (see
/// `SessionFile`).
pub const SESSION_FILE_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often a server checks whether its session file needs to be written
/// again (see `SessionFile::needs_refresh`). It's checked by the wall clock
/// because a timer of `SESSION_FILE_REFRESH_INTERVAL` doesn't count the time
/// the machine was asleep (ex. on Linux and macOS).
pub const SESSION_FILE_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The directory in the cache directory with the session files.
const SESSIONS_DIR_NAME: &str = "editor-sessions";
/// The version of the format of a session file, where a file of another
/// version is ignored, which only means the user is asked again.
const SESSION_FILE_VERSION: u64 = 1;
/// How many times a server tries to take the lock of its session before it
/// says another server uses the session, which is several times because the
/// server that exited unexpectedly may not have released it yet.
const SESSION_LOCK_ATTEMPTS: usize = 10;
/// How long a server waits before it tries to take the lock again.
const SESSION_LOCK_RETRY_DELAY: Duration = Duration::from_millis(50);

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

/// Why a session file can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileOpenError {
  /// The session id isn't valid (see `is_valid_session_id`).
  InvalidId,
  /// Another server that's running uses the session id, because it holds the
  /// session's lock.
  InUse,
}

/// The session file couldn't be written or deleted, so a server that's
/// started again for the session may use what the user decided before (ex.
/// an allow they revoked since).
#[derive(Debug)]
pub struct StaleSessionFileError {
  file_path: PathBuf,
  write_error: String,
  remove_error: std::io::Error,
}

impl std::fmt::Display for StaleSessionFileError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "{}, and failed deleting the older file ({:#}), so the session's decisions from before may come back if dprint restarts. Delete {} to prevent that.",
      self.write_error,
      self.remove_error,
      self.file_path.display()
    )
  }
}

impl std::error::Error for StaleSessionFileError {}

/// The session file of a session id, which the server holds the lock of while
/// the session lasts.
///
/// It's only for that session, so two servers of different sessions never
/// write to the same file. A server whose session id another server that's
/// running uses doesn't use the file (see `SessionFileOpenError::InUse`).
///
/// Every version of dprint that's installed uses the same directory (ex. a
/// project's npm dprint next to a global one), and each one deletes the files
/// the others left there, so the files change by these rules:
///
/// - They're in the `editor-sessions` directory of the cache directory. The
///   file of a session is `<id>.json`, its lock is `<id>.lock`, and a
///   temporary file of writing it is `<id>.<8 lowercase hex digits>.tmp` (see
///   `atomic_write_file`), where the id is 16 to 128 ASCII letters, digits,
///   underscores, and hyphens (see `is_valid_session_id`). Nothing else in
///   the directory has a name of these forms.
/// - A file is a JSON object with a `version` integer of its format, and a
///   server ignores a file of another version, which only means the user is
///   asked again. So a change to the format that an older version can't read
///   raises it.
/// - A server takes an exclusive lock of `<id>.lock` before it uses
///   `<id>.json` and holds it until the session ends, when it deletes both.
///   One that can't take it doesn't use the file, because another server that's
///   running uses the session id.
/// - A server writes its file again when it was last written
///   `SESSION_FILE_REFRESH_INTERVAL` (a day) before by the wall clock, which
///   it checks every `SESSION_FILE_CHECK_INTERVAL` (an hour), and when the
///   clock went back since.
/// - A file is stale when its modification time is more than
///   `STALE_SESSION_FILE_AGE` (a week) before now by the wall clock, or as far
///   after it (ex. it was written while the clock was ahead), so that it's
///   still deleted eventually.
/// - Every version only deletes a file in the directory that has one of the
///   names above, is stale, and whose session's lock it can take (a lock only
///   once the session's `<id>.json` is gone), apart from what a server deletes
///   of its own session. It doesn't look in the directory when it's a
///   symlink.
///
/// So never raise the refresh interval or the check interval to near the
/// stale age, never lower the stale age, and never use another lock or
/// directory.
pub struct SessionFile {
  file_path: PathBuf,
  lock_path: PathBuf,
  /// The lock of the session, which is held until the session ends. It's
  /// `None` once it ended, or when it couldn't be taken because of an error
  /// (ex. the file system doesn't support it), where the file is still used.
  lock: RefCell<Option<FileLock>>,
  /// What was last written or read, so the same state isn't written again.
  last_text: RefCell<Option<String>>,
  /// When the file was last written by the wall clock, which is `None` when
  /// it wasn't written yet.
  last_write_time: Cell<Option<SystemTime>>,
  /// Whether the last write failed, which deleted the file, so it's written
  /// again (see `needs_refresh`).
  write_failed: Cell<bool>,
  /// Whether the session ended, which deleted the file, so it's never written
  /// again (ex. by a request that was still running).
  is_closed: Cell<bool>,
}

impl SessionFile {
  /// Gets the session file of a session id, which takes the session's lock.
  /// It fails when the id isn't valid (see `is_valid_session_id`) or another
  /// server that's running uses it.
  pub fn open(environment: &impl Environment, session_id: &str) -> Result<Self, SessionFileOpenError> {
    // IMPORTANT: The id is part of a file path, so only use one that can't name another file.
    if !is_valid_session_id(session_id) {
      return Err(SessionFileOpenError::InvalidId);
    }
    let dir_path = get_sessions_dir(environment);
    let lock_path = dir_path.join(format!("{}.lock", session_id));
    let lock = lock_session(environment, &dir_path, &lock_path)?;
    Ok(Self {
      file_path: dir_path.join(format!("{}.json", session_id)),
      lock_path,
      lock: RefCell::new(lock),
      last_text: Default::default(),
      last_write_time: Default::default(),
      write_failed: Default::default(),
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
  /// was last written, when the last write failed, or when `force` is `true`
  /// (ex. so that it isn't stale).
  ///
  /// A failure deletes the file, so that a server that's started again for
  /// the session asks the user again instead of using what they decided
  /// before (ex. an allow they revoked), and it's written again later (see
  /// `needs_refresh`). It's an error when the file couldn't be deleted
  /// either, which the user needs to be told about. Otherwise, a failure is
  /// only logged because it only means the user may be asked again.
  pub fn write(&self, environment: &impl Environment, state: &SessionState, force: bool) -> Result<(), StaleSessionFileError> {
    if self.is_closed.get() {
      return Ok(());
    }
    let text = match serde_json::to_string(&SessionFileContent {
      version: SESSION_FILE_VERSION,
      state,
    }) {
      Ok(text) => text,
      Err(err) => return self.forget_failed_write(environment, format!("Failed serializing what was decided for the session: {:#}", err)),
    };
    if !force && !self.write_failed.get() && self.last_text.borrow().as_deref() == Some(text.as_str()) {
      return Ok(());
    }
    let result = match self.file_path.parent() {
      Some(dir_path) => environment.mk_dir_all(dir_path),
      None => Ok(()),
    }
    .and_then(|()| environment.atomic_write_file_bytes(&self.file_path, text.as_bytes()));
    match result {
      Ok(()) => {
        *self.last_text.borrow_mut() = Some(text);
        self.last_write_time.set(Some(environment.sys_time_now()));
        self.write_failed.set(false);
        Ok(())
      }
      Err(err) => self.forget_failed_write(environment, format!("Failed writing {}: {:#}", self.file_path.display(), err)),
    }
  }

  /// Gets whether the file needs to be written again, which is when the last
  /// write failed, when it was last written `SESSION_FILE_REFRESH_INTERVAL`
  /// before by the wall clock, or when the clock went back since (so it isn't
  /// stale for being too far ahead).
  pub fn needs_refresh(&self, environment: &impl Environment) -> bool {
    if self.is_closed.get() {
      return false;
    }
    if self.write_failed.get() {
      return true;
    }
    let Some(last_write_time) = self.last_write_time.get() else {
      return false;
    };
    match environment.sys_time_now().duration_since(last_write_time) {
      Ok(elapsed) => elapsed >= SESSION_FILE_REFRESH_INTERVAL,
      // the clock went back
      Err(_) => true,
    }
  }

  /// Deletes the file once the session ended (ex. the client stopped the
  /// server), which is never written again after. The session's lock is
  /// deleted and released too.
  pub fn delete(&self, environment: &impl Environment) {
    self.is_closed.set(true);
    remove_session_dir_file(environment, &self.file_path);
    // it's deleted while it's held so another server doesn't take it in between
    if let Some(lock) = self.lock.borrow_mut().take() {
      remove_session_dir_file(environment, &self.lock_path);
      drop(lock);
    }
  }

  /// Forgets what was written after a write failed and deletes the file,
  /// which is then outdated.
  fn forget_failed_write(&self, environment: &impl Environment, write_error: String) -> Result<(), StaleSessionFileError> {
    *self.last_text.borrow_mut() = None;
    self.write_failed.set(true);
    match environment.remove_file(&self.file_path) {
      Err(remove_error) if remove_error.kind() != std::io::ErrorKind::NotFound => Err(StaleSessionFileError {
        file_path: self.file_path.clone(),
        write_error,
        remove_error,
      }),
      _ => {
        log_warn!(
          environment,
          "{}. The file was deleted, so the user is asked again if dprint restarts before it's written again.",
          write_error
        );
        Ok(())
      }
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
/// started again). It only deletes the files of the sessions no server that's
/// running uses (see `SessionFile`).
pub fn remove_stale_session_files(environment: &impl Environment) {
  let dir_path = get_sessions_dir(environment);
  // IMPORTANT: Don't follow a symlink, which would delete the files of another directory.
  if environment.path_kind(&dir_path) != Some(PathKind::Dir) {
    return;
  }
  let Ok(entries) = environment.dir_info(&dir_path) else {
    return;
  };
  let now = environment.sys_time_now();
  let mut stale_lock_session_ids = Vec::new();
  for entry in entries {
    let DirEntry::File { path } = entry else {
      continue;
    };
    // IMPORTANT: Only delete the files of sessions, which may be in a directory with others.
    let Some((session_id, kind)) = path.file_name().and_then(|name| name.to_str()).and_then(parse_session_dir_file_name) else {
      continue;
    };
    if !is_stale(environment, &path, now) {
      continue;
    }
    match kind {
      SessionDirFileKind::Session | SessionDirFileKind::Temp => remove_unless_locked(environment, &dir_path.join(format!("{}.lock", session_id)), &path),
      SessionDirFileKind::Lock => stale_lock_session_ids.push(session_id.to_string()),
    }
  }
  // a lock is only deleted once the file of its session is gone
  for session_id in stale_lock_session_ids {
    if !environment.path_exists(dir_path.join(format!("{}.json", session_id))) {
      let lock_path = dir_path.join(format!("{}.lock", session_id));
      remove_unless_locked(environment, &lock_path, &lock_path);
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

/// The kind of a file in the sessions directory (see `SessionFile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionDirFileKind {
  /// `<id>.json`
  Session,
  /// `<id>.<8 lowercase hex digits>.tmp`
  Temp,
  /// `<id>.lock`
  Lock,
}

/// Takes the lock of a session, which fails when another server that's
/// running holds it. It's `None` when it can't be taken because of an error
/// (ex. the file system doesn't support it).
fn lock_session(environment: &impl Environment, dir_path: &Path, lock_path: &Path) -> Result<Option<FileLock>, SessionFileOpenError> {
  if let Err(err) = environment.mk_dir_all(dir_path) {
    log_debug!(environment, "Failed creating {}: {:#}", dir_path.display(), err);
    return Ok(None);
  }
  for attempt in 0..SESSION_LOCK_ATTEMPTS {
    match environment.try_lock_file(lock_path) {
      // a server that deleted a stale lock while this opened it means this holds a lock of a deleted file
      Ok(Some(lock)) if environment.path_exists(lock_path) => return Ok(Some(lock)),
      Ok(Some(_)) => {}
      Ok(None) => {
        if attempt + 1 < SESSION_LOCK_ATTEMPTS {
          environment.thread_sleep(SESSION_LOCK_RETRY_DELAY);
        }
      }
      Err(err) => {
        log_debug!(environment, "Failed locking {}: {:#}", lock_path.display(), err);
        return Ok(None);
      }
    }
  }
  Err(SessionFileOpenError::InUse)
}

/// Gets the session id and the kind of a file in the sessions directory by
/// its name, which is `None` when it isn't the name of one (see
/// `SessionFile`).
fn parse_session_dir_file_name(name: &str) -> Option<(&str, SessionDirFileKind)> {
  // the id doesn't have a dot
  let (session_id, extension) = name.split_once('.')?;
  if !is_valid_session_id(session_id) {
    return None;
  }
  let kind = match extension {
    "json" => SessionDirFileKind::Session,
    "lock" => SessionDirFileKind::Lock,
    _ => {
      let random = extension.strip_suffix(".tmp")?;
      if random.len() != 8 || !random.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
      }
      SessionDirFileKind::Temp
    }
  };
  Some((session_id, kind))
}

/// Gets whether a file in the sessions directory is stale, which is when its
/// modification time is more than `STALE_SESSION_FILE_AGE` from now in either
/// direction. A time ahead of now (ex. it was written while the clock was
/// ahead) is otherwise fresh, and it's still deleted eventually.
fn is_stale(environment: &impl Environment, file_path: &Path, now: SystemTime) -> bool {
  let Ok(modified) = environment.fs_metadata(file_path).and_then(|metadata| metadata.modified()) else {
    return false;
  };
  let age = now.duration_since(modified).unwrap_or_else(|err| err.duration());
  age > STALE_SESSION_FILE_AGE
}

/// Deletes a file of a session unless a server that's running holds the
/// session's lock, which is also kept when it can't be told.
fn remove_unless_locked(environment: &impl Environment, lock_path: &Path, file_path: &Path) {
  match environment.try_lock_file(lock_path) {
    Ok(Some(lock)) => {
      remove_session_dir_file(environment, file_path);
      drop(lock);
    }
    Ok(None) => {}
    Err(err) => log_debug!(environment, "Failed locking {}: {:#}", lock_path.display(), err),
  }
}

/// Deletes a file in the sessions directory, which is fine when it doesn't
/// exist.
fn remove_session_dir_file(environment: &impl Environment, file_path: &Path) {
  if let Err(err) = environment.remove_file(file_path)
    && err.kind() != std::io::ErrorKind::NotFound
  {
    log_debug!(environment, "Failed deleting {}: {:#}", file_path.display(), err);
  }
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
  const DAY_SECS: u64 = 24 * 60 * 60;

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
    assert_eq!(
      SessionFile::open(&environment, "..\\..\\0123456789").err(),
      Some(SessionFileOpenError::InvalidId)
    );
  }

  #[test]
  fn should_read_what_was_written() {
    let environment = TestEnvironment::new();
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    assert_eq!(file.read(&environment), None);
    let state = SessionState {
      allowed_downloads: vec![AllowedDownload {
        download: "https://example.com/plugin.wasm".to_string(),
        item: None,
      }],
      ..Default::default()
    };
    file.write(&environment, &state, false).unwrap();
    // ex. the server exited unexpectedly
    drop(file);
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    assert_eq!(file.read(&environment), Some(state));
  }

  #[test]
  fn should_ignore_a_session_file_with_another_format() {
    let environment = TestEnvironment::new();
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
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
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    file.write(&environment, &SessionState::default(), false).unwrap();
    assert!(environment.path_exists(&file.file_path));
    assert!(environment.path_exists(&file.lock_path));
    file.delete(&environment);
    assert!(!environment.path_exists(&file.file_path));
    // the lock is deleted and released too
    assert!(!environment.path_exists(&file.lock_path));
    assert!(SessionFile::open(&environment, SESSION_ID).is_ok());
    // ex. a request that was still running
    file.write(&environment, &SessionState::default(), true).unwrap();
    assert!(!environment.path_exists(&file.file_path));
    assert!(!file.needs_refresh(&environment));
  }

  #[test]
  fn should_not_use_a_session_file_another_server_uses() {
    let environment = TestEnvironment::new();
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    assert_eq!(SessionFile::open(&environment, SESSION_ID).err(), Some(SessionFileOpenError::InUse));
    // but it's used once that server exited
    drop(file);
    assert!(SessionFile::open(&environment, SESSION_ID).is_ok());
  }

  #[test]
  fn should_delete_the_session_file_when_writing_it_fails() {
    let environment = TestEnvironment::new();
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    let allowed = SessionState {
      allowed_downloads: vec![AllowedDownload {
        download: "https://example.com/plugin.wasm".to_string(),
        item: None,
      }],
      ..Default::default()
    };
    file.write(&environment, &allowed, false).unwrap();
    // revoking the allow fails to be written
    environment.set_file_write_fails(&file.file_path, true);
    file.write(&environment, &SessionState::default(), false).unwrap();
    let messages = environment.take_stderr_messages();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("The file was deleted"), "{}", messages[0]);
    // so the allow isn't used if the server is started again
    assert!(!environment.path_exists(&file.file_path));
    // and it's written again later, even when nothing changed
    assert!(file.needs_refresh(&environment));
    environment.set_file_write_fails(&file.file_path, false);
    file.write(&environment, &SessionState::default(), false).unwrap();
    assert!(!file.needs_refresh(&environment));
    drop(file);
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    assert_eq!(file.read(&environment), Some(SessionState::default()));
  }

  #[test]
  fn should_error_when_a_session_file_that_failed_to_be_written_is_not_deleted() {
    let environment = TestEnvironment::new();
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    file.write(&environment, &SessionState::default(), false).unwrap();
    environment.set_file_write_fails(&file.file_path, true);
    environment.set_file_remove_fails(&file.file_path, true);
    let err = file.write(&environment, &SessionState::default(), true).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("may come back if dprint restarts"), "{}", message);
    assert!(message.contains(&file.file_path.display().to_string()), "{}", message);
    assert!(file.needs_refresh(&environment));
    // it's tried again later
    environment.set_file_write_fails(&file.file_path, false);
    environment.set_file_remove_fails(&file.file_path, false);
    file.write(&environment, &SessionState::default(), false).unwrap();
    assert!(!file.needs_refresh(&environment));
  }

  #[test]
  fn should_refresh_the_session_file_by_the_wall_clock() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    // there's nothing to keep before it's written
    assert!(!file.needs_refresh(&environment));
    file.write(&environment, &SessionState::default(), false).unwrap();
    environment.set_fs_time(1_000_000 + DAY_SECS - 1);
    assert!(!file.needs_refresh(&environment));
    // ex. the machine was asleep, which a timer doesn't count
    environment.set_fs_time(1_000_000 + 9 * DAY_SECS);
    assert!(file.needs_refresh(&environment));
    file.write(&environment, &SessionState::default(), true).unwrap();
    assert!(!file.needs_refresh(&environment));
    // the clock went back, so the file would be ahead of it
    environment.set_fs_time(1_000_000);
    assert!(file.needs_refresh(&environment));
  }

  #[test]
  fn should_remove_stale_session_files() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let stale = SessionFile::open(&environment, "stale-0123456789").unwrap();
    stale.write(&environment, &SessionState::default(), false).unwrap();
    let (stale_file_path, stale_lock_path) = (stale.file_path.clone(), stale.lock_path.clone());
    // ex. its server crashed and wasn't started again
    drop(stale);
    environment.set_fs_time(1_000_000 + STALE_SESSION_FILE_AGE.as_secs());
    let recent = SessionFile::open(&environment, "recent-0123456789").unwrap();
    recent.write(&environment, &SessionState::default(), false).unwrap();
    // it's only stale once it's older than the age
    remove_stale_session_files(&environment);
    assert!(environment.path_exists(&stale_file_path));
    environment.set_fs_time(1_000_001 + STALE_SESSION_FILE_AGE.as_secs());
    remove_stale_session_files(&environment);
    assert!(!environment.path_exists(&stale_file_path));
    assert!(!environment.path_exists(&stale_lock_path));
    assert!(environment.path_exists(&recent.file_path));
    assert!(environment.path_exists(&recent.lock_path));
  }

  #[test]
  fn should_not_remove_the_session_file_of_a_running_server() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let file = SessionFile::open(&environment, SESSION_ID).unwrap();
    file.write(&environment, &SessionState::default(), false).unwrap();
    // ex. the machine was asleep, or the clock jumped ahead
    environment.set_fs_time(1_000_000 + 30 * DAY_SECS);
    remove_stale_session_files(&environment);
    assert!(environment.path_exists(&file.file_path));
    assert!(environment.path_exists(&file.lock_path));
    // until it exits
    drop(file);
    remove_stale_session_files(&environment);
    assert!(!environment.path_exists(get_sessions_dir(&environment).join(format!("{}.json", SESSION_ID))));
  }

  #[test]
  fn should_remove_a_session_file_whose_time_is_far_ahead() {
    let environment = TestEnvironment::new();
    let dir_path = get_sessions_dir(&environment);
    environment.mk_dir_all(&dir_path).unwrap();
    let ahead_path = dir_path.join("ahead-0123456789.json");
    let far_ahead_path = dir_path.join("far-ahead-0123456789.json");
    // written while the clock was ahead
    environment.set_fs_time(1_000_000 + 6 * DAY_SECS);
    environment.write_file(&ahead_path, "{}").unwrap();
    environment.set_fs_time(1_000_000 + 8 * DAY_SECS);
    environment.write_file(&far_ahead_path, "{}").unwrap();
    environment.set_fs_time(1_000_000);
    remove_stale_session_files(&environment);
    assert!(environment.path_exists(&ahead_path));
    assert!(!environment.path_exists(&far_ahead_path));
  }

  #[test]
  fn should_only_remove_stale_files_with_the_names_of_session_files() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let dir_path = get_sessions_dir(&environment);
    environment.mk_dir_all(&dir_path).unwrap();
    let removed = ["0123456789abcdef.json", "0123456789abcdef.0a1b2c3d.tmp", "0123456789abcdef.lock"];
    let kept = [
      "other.json",
      "0123456789abcde.json",
      "0123456789abcdef.txt",
      "0123456789abcdef.json.bak",
      "0123456789abcdef.0A1B2C3D.tmp",
      "0123456789abcdef.0a1b2c3.tmp",
      "notes.md",
    ];
    for name in removed.iter().chain(&kept) {
      environment.write_file(dir_path.join(name), "{}").unwrap();
    }
    environment.set_fs_time(1_000_000 + 8 * DAY_SECS);
    remove_stale_session_files(&environment);
    for name in removed {
      assert!(!environment.path_exists(dir_path.join(name)), "{}", name);
    }
    for name in kept {
      assert!(environment.path_exists(dir_path.join(name)), "{}", name);
    }
  }

  #[test]
  fn should_not_remove_stale_files_in_a_symlinked_sessions_dir() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000_000);
    let other_dir_path = environment.get_cache_dir().join("other");
    environment.mk_dir_all(&other_dir_path).unwrap();
    let file_path = other_dir_path.join("0123456789abcdef.json");
    environment.write_file(&file_path, "{}").unwrap();
    environment.symlink(&other_dir_path, get_sessions_dir(&environment)).unwrap();
    environment.set_fs_time(1_000_000 + 8 * DAY_SECS);
    remove_stale_session_files(&environment);
    assert!(environment.path_exists(&file_path));
  }

  #[test]
  fn should_parse_the_names_of_session_files() {
    assert_eq!(
      parse_session_dir_file_name("0123456789abcdef.json"),
      Some(("0123456789abcdef", SessionDirFileKind::Session))
    );
    assert_eq!(
      parse_session_dir_file_name("0123456789abcdef.lock"),
      Some(("0123456789abcdef", SessionDirFileKind::Lock))
    );
    assert_eq!(
      parse_session_dir_file_name("0123456789abcdef.0a1b2c3d.tmp"),
      Some(("0123456789abcdef", SessionDirFileKind::Temp))
    );
    assert_eq!(parse_session_dir_file_name("0123456789abcdef"), None);
    assert_eq!(parse_session_dir_file_name("0123456789abcdef.tmp"), None);
    assert_eq!(parse_session_dir_file_name(".0123456789abcdef.json"), None);
  }
}
