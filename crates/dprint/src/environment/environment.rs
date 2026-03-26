use anyhow::Result;
use anyhow::bail;
use std::borrow::Cow;
use std::ffi::OsString;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

use dprint_core::async_runtime::async_trait;
use sys_traits::BaseFsCreateDir;
use sys_traits::BaseFsMetadata;
use sys_traits::BaseFsOpen;
use sys_traits::BaseFsRead;
use sys_traits::BaseFsRemoveFile;
use sys_traits::BaseFsRename;
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
  async fn download_file_no_redirects(&self, url: &Url) -> Result<Option<DownloadedFile>>;

  /// Downloads a file, following redirects, and returns `None` on 404.
  async fn download_file<'a>(&self, url: &'a Url) -> Result<(Cow<'a, Url>, Option<DownloadedFile>)> {
    let mut current_url = Cow::Borrowed(url);
    for _ in 0..=10 {
      let result = match self.download_file_no_redirects(&current_url).await? {
        Some(r) => r,
        None => return Ok((current_url, None)),
      };
      if let Some(location) = result.headers.get("location") {
        current_url = Cow::Owned(current_url.join(location)?);
        continue;
      }
      return Ok((current_url, Some(result)));
    }
    bail!("Too many redirects for {}", url)
  }

  /// Downloads a file, following redirects, and errors when not found.
  async fn download_file_err_404<'a>(&self, url: &'a Url) -> Result<(Cow<'a, Url>, DownloadedFile)> {
    match self.download_file(url).await {
      Ok((url, Some(value))) => Ok((url, value)),
      Ok((url, None)) => bail!("Error downloading {} - 404 Not Found", url),
      Err(err) => Err(err),
    }
  }
}

#[async_trait]
pub trait Environment:
  Clone
  + Send
  + Sync
  + std::fmt::Debug
  + UrlDownloader
  + BaseFsCreateDir
  + BaseFsMetadata
  + BaseFsOpen
  + BaseFsRead
  + BaseFsRemoveFile
  + BaseFsRename
  + ThreadSleep
  + SystemRandom
  + SystemTimeNow
  + 'static
{
  fn is_real(&self) -> bool;

  fn env_var(&self, name: &str) -> Option<OsString>;

  fn get_staged_files(&self) -> Result<Vec<PathBuf>>;
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
    deno_path_util::fs::atomic_write_file_with_retries(self, file_path.as_ref(), bytes, 0o644)
  }
  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> io::Result<()>;
  fn remove_file(&self, file_path: impl AsRef<Path>) -> io::Result<()>;
  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> io::Result<()>;
  fn dir_info(&self, dir_path: impl AsRef<Path>) -> io::Result<Vec<DirEntry>>;
  fn path_exists(&self, path: impl AsRef<Path>) -> bool;
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
  fn max_threads(&self) -> usize;
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
