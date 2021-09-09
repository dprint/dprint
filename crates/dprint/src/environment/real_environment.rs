use dprint_cli_core::download_url;
use dprint_cli_core::logging::{log_action_with_progress, show_confirm, show_multi_select, show_select, Logger, ProgressBars};
use dprint_core::types::ErrBox;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{DirEntry, DirEntryKind, Environment};
use crate::plugins::CompilationResult;

#[derive(Clone)]
pub struct RealEnvironment {
  logger: Logger,
  progress_bars: Option<ProgressBars>,
  is_verbose: bool,
}

impl RealEnvironment {
  pub fn new(is_verbose: bool, is_silent: bool) -> Result<RealEnvironment, ErrBox> {
    let logger = Logger::new("dprint", is_silent);
    let progress_bars = if is_silent { None } else { ProgressBars::new(&logger) };
    let environment = RealEnvironment {
      logger,
      progress_bars,
      is_verbose,
    };

    // ensure the cache directory is created
    if let Err(err) = environment.mk_dir_all(&get_cache_dir()?) {
      return err!("Error creating cache directory: {:?}", err);
    }

    Ok(environment)
  }
}

impl Environment for RealEnvironment {
  fn is_real(&self) -> bool {
    true
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String, ErrBox> {
    Ok(String::from_utf8(self.read_file_bytes(file_path)?)?)
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>, ErrBox> {
    log_verbose!(self, "Reading file: {}", file_path.as_ref().display());
    match fs::read(&file_path) {
      Ok(bytes) => Ok(bytes),
      Err(err) => err!("Error reading file {}: {}", file_path.as_ref().display(), err.to_string()),
    }
  }

  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<(), ErrBox> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), ErrBox> {
    log_verbose!(self, "Writing file: {}", file_path.as_ref().display());
    match fs::write(&file_path, bytes) {
      Ok(_) => Ok(()),
      Err(err) => err!("Error writing file {}: {}", file_path.as_ref().display(), err.to_string()),
    }
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<(), ErrBox> {
    log_verbose!(self, "Deleting file: {}", file_path.as_ref().display());
    match fs::remove_file(&file_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(err) => err!("Error deleting file {}: {}", file_path.as_ref().display(), err.to_string()),
    }
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<(), ErrBox> {
    log_verbose!(self, "Deleting directory: {}", dir_path.as_ref().display());
    match fs::remove_dir_all(&dir_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(err) => err!("Error removing directory {}: {}", dir_path.as_ref().display(), err.to_string()),
    }
  }

  fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox> {
    log_verbose!(self, "Downloading url: {}", url);

    download_url(url, &self.progress_bars, |env_var_name| std::env::var(env_var_name).ok())
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>, ErrBox> {
    let mut entries = Vec::new();

    let dir_info = match std::fs::read_dir(&dir_path) {
      Ok(result) => result,
      Err(err) => {
        if is_system_volume_error(dir_path.as_ref(), &err) {
          return Ok(Vec::with_capacity(0));
        } else {
          return Err(Box::new(err));
        }
      }
    };

    for entry in dir_info {
      let entry = entry?;
      let file_type = entry.file_type()?;
      if file_type.is_dir() {
        entries.push(DirEntry {
          kind: DirEntryKind::Directory,
          path: entry.path().to_path_buf(),
        });
      } else if file_type.is_file() {
        entries.push(DirEntry {
          kind: DirEntryKind::File,
          path: entry.path().to_path_buf(),
        });
      }
    }

    Ok(entries)
  }

  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool {
    log_verbose!(self, "Checking path exists: {}", file_path.as_ref().display());
    file_path.as_ref().exists()
  }

  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, ErrBox> {
    // use this to avoid //?//C:/etc... like paths on windows (UNC)
    Ok(dunce::canonicalize(path)?)
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    path.as_ref().is_absolute()
  }

  fn mk_dir_all(&self, path: impl AsRef<Path>) -> Result<(), ErrBox> {
    log_verbose!(self, "Creating directory: {}", path.as_ref().display());
    match fs::create_dir_all(&path) {
      Ok(_) => Ok(()),
      Err(err) => err!("Error creating directory {}: {}", path.as_ref().display(), err.to_string()),
    }
  }

  fn cwd(&self) -> PathBuf {
    std::env::current_dir().expect("Expected to get the current working directory.")
  }

  fn log(&self, text: &str) {
    self.logger.log(text, "dprint");
  }

  fn log_silent(&self, text: &str) {
    self.logger.log_bypass_silent(text, "dprint");
  }

  fn log_stderr_with_context(&self, text: &str, context_name: &str) {
    self.logger.log_err(text, context_name);
  }

  fn log_action_with_progress<
    TResult: std::marker::Send + std::marker::Sync,
    TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
  >(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult {
    log_action_with_progress(&self.progress_bars, message, action, total_size)
  }

  fn get_cache_dir(&self) -> PathBuf {
    // this would have errored in the constructor so it's ok to unwrap here
    get_cache_dir().unwrap()
  }

  fn get_time_secs(&self) -> u64 {
    SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
  }

  fn get_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<String>) -> Result<usize, ErrBox> {
    show_select(&self.logger, "dprint", prompt_message, item_indent_width, items)
  }

  fn get_multi_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<(bool, String)>) -> Result<Vec<usize>, ErrBox> {
    show_multi_select(
      &self.logger,
      "dprint",
      prompt_message,
      item_indent_width,
      items.iter().map(|(value, text)| (value.to_owned(), text)).collect(),
    )
  }

  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool, ErrBox> {
    show_confirm(&self.logger, "dprint", prompt_message, default_value)
  }

  fn get_terminal_width(&self) -> u16 {
    dprint_cli_core::terminal::get_terminal_width().unwrap_or(60)
  }

  #[inline]
  fn is_verbose(&self) -> bool {
    self.is_verbose
  }

  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
    crate::plugins::compile_wasm(wasm_bytes)
  }

  fn stdout(&self) -> Box<dyn std::io::Write + Send> {
    Box::new(std::io::stdout())
  }

  fn stdin(&self) -> Box<dyn std::io::Read + Send> {
    Box::new(std::io::stdin())
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<(), ErrBox> {
    // from bvm (https://github.com/bvm/bvm)
    use winreg::{enums::*, RegKey};
    log_verbose!(self, "Ensuring '{}' is on the path.", directory_path);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = hkcu.create_subkey("Environment")?;
    let mut path: String = env.get_value("Path")?;

    // add to the path if it doesn't have this entry
    if !path.split(";").any(|p| p == directory_path) {
      if !path.is_empty() && !path.ends_with(';') {
        path.push_str(";")
      }
      path.push_str(&directory_path);
      env.set_value("Path", &path)?;
    }
    Ok(())
  }

  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<(), ErrBox> {
    // from bvm (https://github.com/bvm/bvm)
    use winreg::{enums::*, RegKey};
    log_verbose!(self, "Ensuring '{}' is on the path.", directory_path);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = hkcu.create_subkey("Environment")?;
    let path: String = env.get_value("Path")?;
    let mut paths = path.split(";").collect::<Vec<_>>();
    let original_len = paths.len();

    paths.retain(|p| p != &directory_path);

    let was_removed = original_len != paths.len();
    if was_removed {
      env.set_value("Path", &paths.join(";"))?;
    }
    Ok(())
  }
}

const CACHE_DIR_ENV_VAR_NAME: &str = "DPRINT_CACHE_DIR";

fn get_cache_dir() -> Result<PathBuf, ErrBox> {
  get_cache_dir_internal(|var_name| std::env::var(var_name).ok())
}

fn get_cache_dir_internal(get_env_var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ErrBox> {
  if let Some(dir_path) = get_env_var(CACHE_DIR_ENV_VAR_NAME) {
    if !dir_path.trim().is_empty() {
      let dir_path = PathBuf::from(dir_path);
      // seems dangerous to allow a relative path as this directory may be deleted
      return if !dir_path.is_absolute() {
        err!("The {} environment variable must specify an absolute path.", CACHE_DIR_ENV_VAR_NAME)
      } else {
        Ok(dir_path)
      };
    }
  }

  match dirs::cache_dir() {
    Some(dir) => Ok(dir.join("dprint").join("cache")),
    None => err!("Expected to find cache directory"),
  }
}

fn is_system_volume_error(dir_path: &Path, err: &std::io::Error) -> bool {
  // ignore any access denied errors for the system volume information
  cfg!(target_os = "windows")
    && matches!(err.raw_os_error(), Some(5))
    && matches!(dir_path.file_name().map(|f| f.to_str()).flatten(), Some("System Volume Information"))
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_get_cache_dir_based_on_env_var() {
    let default_dir = dirs::cache_dir().unwrap().join("dprint").join("cache");
    let value = if cfg!(target_os = "windows") {
      "C:/.dprint-cache"
    } else {
      "/home/david/.dprint-cache"
    };
    assert_eq!(get_cache_dir_internal(|_| Some(value.to_string())).unwrap().to_string_lossy(), value);
    assert_eq!(get_cache_dir_internal(|_| Some("".to_string())).unwrap(), default_dir);
    assert_eq!(get_cache_dir_internal(|_| Some("  ".to_string())).unwrap(), default_dir);
    assert_eq!(get_cache_dir_internal(|_| None).unwrap(), default_dir);
  }

  #[test]
  fn should_error_when_cache_dir_env_var_relative() {
    let result = get_cache_dir_internal(|_| Some("./dir".to_string())).err();
    assert_eq!(
      result.unwrap().to_string(),
      "The DPRINT_CACHE_DIR environment variable must specify an absolute path."
    );
  }
}
