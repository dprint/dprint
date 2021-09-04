use dprint_core::types::ErrBox;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::plugins::CompilationResult;

#[derive(Debug)]
pub struct DirEntry {
  pub kind: DirEntryKind,
  pub path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum DirEntryKind {
  Directory,
  File,
}

pub trait Environment: Clone + std::marker::Send + std::marker::Sync + 'static {
  fn is_real(&self) -> bool;
  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String, ErrBox>;
  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>, ErrBox>;
  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<(), ErrBox>;
  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), ErrBox>;
  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<(), ErrBox>;
  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<(), ErrBox>;
  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>, ErrBox>;
  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool;
  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, ErrBox>;
  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool;
  fn mk_dir_all(&self, path: impl AsRef<Path>) -> Result<(), ErrBox>;
  fn cwd(&self) -> PathBuf;
  fn log(&self, text: &str);
  fn log_stderr(&self, text: &str) {
    self.log_stderr_with_context(text, "dprint");
  }
  /// Logs an error to the console providing the context name.
  /// This will cause the logger to output the context name when appropriate.
  /// Ex. Will log the dprint process plugin name.
  fn log_stderr_with_context(&self, text: &str, context_name: &str);
  /// Information to output when logging is silent.
  fn log_silent(&self, text: &str);
  fn log_action_with_progress<
    TResult: std::marker::Send + std::marker::Sync,
    TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
  >(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult;
  fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox>;
  fn get_cache_dir(&self) -> PathBuf;
  fn get_time_secs(&self) -> u64;
  fn get_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<String>) -> Result<usize, ErrBox>;
  fn get_multi_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<(bool, String)>) -> Result<Vec<usize>, ErrBox>;
  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool, ErrBox>;
  fn get_terminal_width(&self) -> u16;
  fn is_verbose(&self) -> bool;
  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox>;
  fn stdout(&self) -> Box<dyn Write + Send>;
  fn stdin(&self) -> Box<dyn Read + Send>;
  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<(), ErrBox>;
  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<(), ErrBox>;
}

// use a macro here so the expression provided is only evaluated when in verbose mode
macro_rules! log_verbose {
    ($environment:expr, $($arg:tt)*) => {
        if $environment.is_verbose() {
            let mut text = String::from("[VERBOSE]: ");
            text.push_str(&format!($($arg)*));
            $environment.log_stderr(&text);
        }
    }
}
