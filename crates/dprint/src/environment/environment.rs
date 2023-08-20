use anyhow::bail;
use anyhow::Result;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use dprint_core::async_runtime::async_trait;

use crate::plugins::CompilationResult;
use crate::utils::ProgressBars;

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

#[async_trait(?Send)]
pub trait UrlDownloader {
  async fn download_file(&self, url: &str) -> Result<Option<Vec<u8>>>;
  async fn download_file_err_404(&self, url: &str) -> Result<Vec<u8>> {
    match self.download_file(url).await? {
      Some(result) => Ok(result),
      None => bail!("Error downloading {} - 404 Not Found", url),
    }
  }
}

pub trait Environment: Clone + Send + Sync + UrlDownloader + 'static {
  fn is_real(&self) -> bool;
  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String>;
  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>>;
  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<()> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }
  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<()>;
  /// An atomic write, which will write to a temporary file and then rename it to the destination.
  fn atomic_write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    // lifted from https://github.com/denoland/deno/blob/0f4051a37ad23377091043206e64126003caa480/cli/util/fs.rs#L29
    let rand: String = (0..4).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    let extension = format!("{rand}.tmp");
    let tmp_file = file_path.as_ref().with_extension(extension);
    self.write_file_bytes(&tmp_file, bytes)?;
    self.rename(tmp_file, file_path)
  }
  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> Result<()>;
  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<()>;
  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<()>;
  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>>;
  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool;
  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<CanonicalizedPathBuf>;
  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool;
  fn file_permissions(&self, path: impl AsRef<Path>) -> Result<FilePermissions>;
  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> Result<()>;
  fn mk_dir_all(&self, path: impl AsRef<Path>) -> Result<()>;
  fn cwd(&self) -> CanonicalizedPathBuf;
  fn current_exe(&self) -> Result<PathBuf>;
  fn log(&self, text: &str);
  fn log_stderr(&self, text: &str) {
    self.log_stderr_with_context(text, "dprint");
  }
  /// Logs an error to the console providing the context name.
  /// This will cause the logger to output the context name when appropriate.
  /// Ex. Will log the dprint process plugin name.
  fn log_stderr_with_context(&self, text: &str, context_name: &str);
  /// Information to force output when the environment is in "machine readable mode".
  fn log_machine_readable(&self, text: &str);
  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult;
  fn get_cache_dir(&self) -> CanonicalizedPathBuf;
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
  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool>;
  fn is_verbose(&self) -> bool;
  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult>;
  fn wasm_cache_key(&self) -> String;
  fn stdout(&self) -> Box<dyn Write + Send>;
  fn stdin(&self) -> Box<dyn Read + Send>;
  fn progress_bars(&self) -> Option<ProgressBars> {
    None
  }
  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<()>;
  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<()>;
}

// use a macro here so the expression provided is only evaluated when in verbose mode
macro_rules! log_verbose {
  ($logger:expr, $($arg:tt)*) => {
    if $logger.is_verbose() {
      let mut text = String::from("[VERBOSE] ");
      text.push_str(&format!($($arg)*));
      $logger.log_stderr(&text);
    }
  }
}
