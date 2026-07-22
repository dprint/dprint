use anyhow::Result;
use anyhow::bail;
use std::borrow::Cow;
use std::ffi::OsString;
use std::io;
use std::io::Read;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

use dprint_core::async_runtime::async_trait;
use sys_traits::BaseEnvVar;
use sys_traits::BaseFsCreateDir;
use sys_traits::BaseFsMetadata;
use sys_traits::BaseFsOpen;
use sys_traits::BaseFsRead;
use sys_traits::BaseFsRemoveFile;
use sys_traits::BaseFsRename;
use sys_traits::BaseFsSetPermissions;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;

use crate::plugins::CompilationResult;
use crate::utils::BasicShowConfirmStrategy;
use crate::utils::LogLevel;
use crate::utils::ProgressBars;
use crate::utils::ShowConfirmStrategy;

use super::CanonicalizedPathBuf;

#[derive(Debug)]
pub enum DirEntry {
  Directory(PathBuf),
  File { name: std::ffi::OsString, path: PathBuf },
}

/// The kind of entry at a path on the file system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
  File,
  Dir,
  Symlink,
}

#[derive(Debug, Clone)]
pub enum FilePermissions {
  Std(std::fs::Permissions),
  #[allow(dead_code)]
  Test(TestFilePermissions),
}

impl FilePermissions {
  pub fn readonly(&self) -> bool {
    match self {
      FilePermissions::Std(p) => p.readonly(),
      FilePermissions::Test(p) => p.readonly,
    }
  }
}

#[derive(Default, Debug, Clone)]
pub struct TestFilePermissions {
  pub readonly: bool,
}

pub struct DownloadedFile {
  pub headers: std::collections::HashMap<String, String>,
  pub content: Vec<u8>,
}

#[async_trait(?Send)]
pub trait UrlDownloader {
  /// Downloads a file without following redirects. Returns the raw response
  /// headers and content. A redirect response will have a `location` header
  /// and empty content.
  async fn download_file_no_redirects(&self, url: &Url, auth: Option<&str>) -> Result<Option<DownloadedFile>>;

  /// Downloads a file, following redirects, and returns `None` on 404.
  async fn download_file<'a>(&self, url: &'a Url, auth: Option<&str>) -> Result<(Cow<'a, Url>, Option<DownloadedFile>)> {
    let original_origin = (url.scheme().to_string(), url.host_str().map(|h| h.to_string()), url.port_or_known_default());
    let mut current_url = Cow::Borrowed(url);
    let mut current_auth = auth;
    for _ in 0..=10 {
      let result = match self.download_file_no_redirects(&current_url, current_auth).await? {
        Some(r) => r,
        None => return Ok((current_url, None)),
      };
      if let Some(location) = result.headers.get("location") {
        current_url = Cow::Owned(current_url.join(location)?);
        // drop the auth on a cross-origin redirect
        let new_origin = (
          current_url.scheme().to_string(),
          current_url.host_str().map(|h| h.to_string()),
          current_url.port_or_known_default(),
        );
        if new_origin != original_origin {
          current_auth = None;
        }
        continue;
      }
      return Ok((current_url, Some(result)));
    }
    bail!("Too many redirects for {}", url)
  }

  /// Downloads a file, following redirects, and errors when not found.
  async fn download_file_err_404<'a>(&self, url: &'a Url, auth: Option<&str>) -> Result<(Cow<'a, Url>, DownloadedFile)> {
    match self.download_file(url, auth).await {
      Ok((url, Some(value))) => Ok((url, value)),
      Ok((url, None)) => bail!("Error downloading {} - 404 Not Found", url),
      Err(err) => Err(err),
    }
  }
}

// use a macro here so the expression provided is only evaluated when in debug mode
macro_rules! log_debug {
  ($logger:expr, $($arg:tt)*) => {
    if $logger.log_level().is_debug() {
      let mut text = String::from("[DEBUG] ");
      text.push_str(&format!($($arg)*));
      $logger.__log_stderr__(&text);
    }
  }
}

macro_rules! log_stderr_info {
  ($logger:expr, $single_arg:expr $(,)? ) => {
    if $logger.log_level().is_info() {
      $logger.__log_stderr__($single_arg);
    }
  };
  ($logger:expr, $($arg:tt)*) => {
    if $logger.log_level().is_info() {
      $logger.__log_stderr__(&format!($($arg)*));
    }
  }
}

macro_rules! log_stdout_info {
  ($logger:expr, $single_arg:expr $(,)? ) => {
    if $logger.log_level().is_info() {
      $logger.__log__($single_arg);
    }
  };
  ($logger:expr, $($arg:tt)*) => {
    if $logger.log_level().is_info() {
      $logger.__log__(&format!($($arg)*));
    }
  }
}

macro_rules! log_warn {
  ($logger:expr, $single_arg:expr $(,)? ) => {
    if $logger.log_level().is_warn() {
      $logger.__log_stderr__($single_arg);
    }
  };
  ($logger:expr, $($arg:tt)*) => {
    if $logger.log_level().is_warn() {
      $logger.__log_stderr__(&format!($($arg)*));
    }
  }
}

macro_rules! log_error {
  ($logger:expr, $single_arg:expr $(,)? ) => {
    if $logger.log_level().is_error() {
      $logger.__log_stderr__($single_arg);
    }
  };
  ($logger:expr, $($arg:tt)*) => {
    if $logger.log_level().is_error() {
      $logger.__log_stderr__(&format!($($arg)*));
    }
  }
}

macro_rules! log_all {
  ($logger:expr, $single_arg:expr $(,)? ) => {
    $logger.__log_stderr__($single_arg);
  };
  ($logger:expr, $($arg:tt)*) => {
    $logger.__log_stderr__(&format!($($arg)*));
  }
}

#[async_trait]
pub trait Environment:
  Clone
  + Send
  + Sync
  + std::fmt::Debug
  + UrlDownloader
  + BaseEnvVar
  + BaseFsCreateDir
  + BaseFsMetadata
  + BaseFsOpen
  + BaseFsRead
  + BaseFsRemoveFile
  + BaseFsRename
  + BaseFsSetPermissions
  + ThreadSleep
  + SystemRandom
  + SystemTimeNow
  + 'static
{
  fn is_real(&self) -> bool;

  fn env_var(&self, name: &str) -> Option<OsString>;

  fn get_staged_files(&self) -> Result<Vec<PathBuf>>;
  /// Resolves the files that have uncommitted changes in the git working
  /// directory: staged, unstaged, and untracked (but not gitignored) files.
  fn get_dirty_files(&self) -> Result<Vec<PathBuf>>;
  /// Resolves the path to git's global excludes file (the `core.excludesFile`
  /// config value, falling back to `$XDG_CONFIG_HOME/git/ignore`). Used only when
  /// global gitignore support is opted into via `DPRINT_GLOBAL_GITIGNORE`. The
  /// path is not guaranteed to exist; the caller handles a missing file when
  /// reading it. Returns `None` only when no path can be resolved at all.
  fn global_gitignore_path(&self) -> Option<PathBuf>;
  fn read_file(&self, file_path: impl AsRef<Path>) -> io::Result<String>;
  fn maybe_read_file(&self, file_path: impl AsRef<Path>) -> io::Result<Option<String>> {
    match self.read_file(file_path) {
      Ok(value) => Ok(Some(value)),
      Err(err) => match err.kind() {
        std::io::ErrorKind::NotFound => Ok(None),
        _ => Err(err),
      },
    }
  }
  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> io::Result<Vec<u8>>;
  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> io::Result<()> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }
  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()>;
  /// An atomic write, which will write to a temporary file and then rename it to the destination.
  fn atomic_write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    crate::utils::fs::atomic_write_file_with_retries(self, file_path.as_ref(), bytes, 0o644)
  }
  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> io::Result<()>;
  fn remove_file(&self, file_path: impl AsRef<Path>) -> io::Result<()>;
  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> io::Result<()>;
  /// Removes a directory and all of its contents as best-effort cleanup,
  /// logging any failure at debug level rather than returning it. Use this
  /// for cleanup that shouldn't abort the surrounding operation, while still
  /// surfacing the cause in `--log-level=debug` output.
  fn try_remove_dir_all(&self, dir_path: impl AsRef<Path>) {
    if let Err(err) = self.remove_dir_all(dir_path) {
      log_debug!(self, "{:#}", err);
    }
  }
  fn dir_info(&self, dir_path: impl AsRef<Path>) -> io::Result<Vec<DirEntry>>;
  /// Kills any running process whose executable lives under the given directory
  /// and returns how many were killed. Used when clearing the cache so a process
  /// plugin that's still running can't stop its executable from being deleted
  /// (e.g. on Windows a running executable can't be removed). This is best-effort
  /// and never fails.
  fn kill_processes_using_dir(&self, dir_path: impl AsRef<Path>) -> usize;
  /// Gets whether anything exists at the path (a broken symlink counts as
  /// existing).
  fn path_exists(&self, path: impl AsRef<Path>) -> bool {
    self.path_kind(path).is_some()
  }
  /// Gets whether the path exists and is a file (follows symlinks).
  fn path_is_file(&self, path: impl AsRef<Path>) -> bool;
  /// Stats the path in a single call, saying whether it's a file, directory,
  /// or symlink (`None` when the path doesn't exist). Symlinks are not
  /// followed—canonicalize and stat again to see what a symlink points at.
  fn path_kind(&self, path: impl AsRef<Path>) -> Option<PathKind>;
  fn canonicalize(&self, path: impl AsRef<Path>) -> io::Result<CanonicalizedPathBuf>;
  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool;
  fn file_permissions(&self, path: impl AsRef<Path>) -> io::Result<FilePermissions>;
  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> io::Result<()>;
  fn mk_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()>;
  fn cwd(&self) -> CanonicalizedPathBuf;
  fn current_exe(&self) -> io::Result<PathBuf>;
  /// Don't ever call this directly in the code. That's why this has this weird name.
  fn __log__(&self, text: &str);
  /// Don't ever call this directly in the code. That's why this has this weird name.
  fn __log_stderr__(&self, text: &str) {
    self.log_stderr_with_context(text, "dprint");
  }
  /// Logs an error to the console providing the context name.
  /// This will cause the logger to output the context name when appropriate.
  /// Ex. Will log the dprint process plugin name.
  fn log_stderr_with_context(&self, text: &str, context_name: &str);
  /// Information to force output when the environment is in "machine readable mode".
  fn log_machine_readable(&self, bytes: &[u8]);
  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult;
  fn get_cache_dir(&self) -> CanonicalizedPathBuf;
  fn get_config_dir(&self) -> Option<PathBuf>;
  fn get_home_dir(&self) -> Option<CanonicalizedPathBuf>;
  /// Gets the CPU architecture.
  fn cpu_arch(&self) -> String;
  /// Gets the operating system.
  fn os(&self) -> String;
  fn available_parallelism(&self) -> Option<NonZeroUsize>;
  fn max_threads(&self) -> usize {
    resolve_max_threads(
      self.env_var("DPRINT_MAX_THREADS").as_deref().and_then(|s| s.to_str()),
      self.available_parallelism(),
    )
  }
  /// Gets the CLI version
  fn cli_version(&self) -> String;
  fn get_time_secs(&self) -> u64;
  fn get_selection(&self, prompt_message: &str, item_indent_width: u16, items: &[String]) -> Result<usize>;
  fn get_multi_selection(&self, prompt_message: &str, item_indent_width: u16, items: &[(bool, String)]) -> Result<Vec<usize>>;
  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool> {
    self.confirm_with_strategy(&BasicShowConfirmStrategy {
      prompt: prompt_message,
      default_value,
    })
  }
  fn confirm_with_strategy(&self, strategy: &dyn ShowConfirmStrategy) -> Result<bool>;
  fn run_command_get_status(&self, args: Vec<OsString>) -> io::Result<Option<i32>>;
  fn is_ci(&self) -> bool;
  fn is_terminal_interactive(&self) -> bool;
  fn log_level(&self) -> LogLevel;
  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult>;
  fn wasm_cache_key(&self) -> String;
  /// Returns the current CPU usage as a value from 0-100.
  async fn cpu_usage(&self) -> u8;
  fn stdout(&self) -> Box<dyn Write + Send>;
  fn stdin(&self) -> Box<dyn Read + Send>;
  fn progress_bars(&self) -> Option<&Arc<ProgressBars>> {
    None
  }
  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> io::Result<()>;
  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> io::Result<()>;
}

fn resolve_max_threads(env_var: Option<&str>, available_parallelism: Option<NonZeroUsize>) -> usize {
  fn maybe_specified_threads(env_var: Option<&str>) -> Option<usize> {
    let value = env_var?.parse::<usize>().ok()?;
    if value > 0 { Some(value) } else { None }
  }

  let maybe_actual_count = available_parallelism.map(|p| p.get());
  match maybe_specified_threads(env_var) {
    Some(specified_count) => match maybe_actual_count {
      Some(actual_count) if specified_count > actual_count => actual_count,
      _ => specified_count,
    },
    _ => maybe_actual_count.unwrap_or(4),
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_resolve_num_threads() {
    assert_eq!(resolve_max_threads(None, None), 4);
    assert_eq!(resolve_max_threads(None, NonZeroUsize::new(1)), 1);
    assert_eq!(resolve_max_threads(None, NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("2"), NonZeroUsize::new(4)), 2);
    assert_eq!(resolve_max_threads(Some("0"), NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("5"), NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("4"), NonZeroUsize::new(4)), 4);
  }
}
