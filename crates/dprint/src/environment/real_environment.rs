use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use once_cell::sync::Lazy;
use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use super::CanonicalizedPathBuf;
use super::DirEntry;
use super::DirEntryKind;
use super::Environment;
use super::FilePermissions;
use super::UrlDownloader;
use crate::plugins::CompilationResult;
use crate::utils::log_action_with_progress;
use crate::utils::show_confirm;
use crate::utils::show_multi_select;
use crate::utils::show_select;
use crate::utils::Logger;
use crate::utils::LoggerOptions;
use crate::utils::ProgressBars;
use crate::utils::RealUrlDownloader;

pub struct RealEnvironmentOptions {
  pub is_verbose: bool,
  pub is_stdout_machine_readable: bool,
  pub runtime_handle: Arc<tokio::runtime::Handle>,
}

#[derive(Clone)]
pub struct RealEnvironment {
  progress_bars: Option<ProgressBars>,
  runtime_handle: Arc<tokio::runtime::Handle>,
  url_downloader: RealUrlDownloader,
  logger: Logger,
}

impl RealEnvironment {
  pub fn new(options: RealEnvironmentOptions) -> Result<RealEnvironment> {
    let logger = Logger::new(&LoggerOptions {
      initial_context_name: "dprint".to_string(),
      is_stdout_machine_readable: options.is_stdout_machine_readable,
      is_verbose: options.is_verbose,
    });
    let progress_bars = ProgressBars::new(&logger);
    let url_downloader = RealUrlDownloader::new(progress_bars.clone(), logger.clone(), |env_var_name| std::env::var(env_var_name).ok())?;
    let environment = RealEnvironment {
      url_downloader,
      logger,
      progress_bars,
      runtime_handle: options.runtime_handle,
    };

    // ensure the cache directory is created
    match (*CACHE_DIR).as_ref() {
      Ok(cache_dir) => cache_dir,
      Err(err) => {
        bail!("Error creating cache directory: {:#}", err);
      }
    };

    Ok(environment)
  }
}

impl UrlDownloader for RealEnvironment {
  fn download_file(&self, url: &str) -> Result<Option<Vec<u8>>> {
    log_verbose!(self, "Downloading url: {}", url);

    self.url_downloader.download(url)
  }
}

impl Environment for RealEnvironment {
  fn is_real(&self) -> bool {
    true
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String> {
    Ok(String::from_utf8(self.read_file_bytes(file_path)?)?)
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>> {
    log_verbose!(self, "Reading file: {}", file_path.as_ref().display());
    match fs::read(&file_path) {
      Ok(bytes) => Ok(bytes),
      Err(err) => bail!("Error reading file {}: {:#}", file_path.as_ref().display(), err),
    }
  }

  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<()> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    log_verbose!(self, "Writing file: {}", file_path.as_ref().display());
    match fs::write(&file_path, bytes) {
      Ok(_) => Ok(()),
      Err(err) => bail!("Error writing file {}: {:#}", file_path.as_ref().display(), err),
    }
  }

  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> Result<()> {
    fs::rename(&path_from, &path_to).with_context(|| format!("Error renaming {} to {}", path_from.as_ref().display(), path_to.as_ref().display()))
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<()> {
    log_verbose!(self, "Deleting file: {}", file_path.as_ref().display());
    match fs::remove_file(&file_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(err) => bail!("Error deleting file {}: {:#}", file_path.as_ref().display(), err),
    }
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<()> {
    log_verbose!(self, "Deleting directory: {}", dir_path.as_ref().display());
    match fs::remove_dir_all(&dir_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(err) => bail!("Error removing directory {}: {:#}", dir_path.as_ref().display(), err),
    }
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>> {
    let mut entries = Vec::new();

    let dir_info = match std::fs::read_dir(&dir_path) {
      Ok(result) => result,
      Err(err) => {
        if is_system_volume_error(dir_path.as_ref(), &err) {
          return Ok(Vec::with_capacity(0));
        } else {
          return Err(err.into());
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

  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<CanonicalizedPathBuf> {
    canonicalize_path(path)
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    path.as_ref().is_absolute()
  }

  fn file_permissions(&self, path: impl AsRef<Path>) -> Result<FilePermissions> {
    Ok(FilePermissions::Std(
      fs::metadata(&path)
        .with_context(|| format!("Error getting file permissions for: {}", path.as_ref().display()))?
        .permissions(),
    ))
  }

  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> Result<()> {
    let permissions = match permissions {
      FilePermissions::Std(p) => p,
      _ => panic!("Programming error. Permissions did not contain an std permission."),
    };
    fs::set_permissions(&path, permissions).with_context(|| format!("Error setting file permissions for: {}", path.as_ref().display()))?;
    Ok(())
  }

  fn mk_dir_all(&self, path: impl AsRef<Path>) -> Result<()> {
    log_verbose!(self, "Creating directory: {}", path.as_ref().display());
    match fs::create_dir_all(&path) {
      Ok(_) => Ok(()),
      Err(err) => bail!("Error creating directory {}: {:#}", path.as_ref().display(), err),
    }
  }

  fn cwd(&self) -> CanonicalizedPathBuf {
    self
      .canonicalize(std::env::current_dir().expect("Expected to get the current working directory."))
      .expect("expected to canonicalize the cwd")
  }

  fn current_exe(&self) -> Result<PathBuf> {
    std::env::current_exe().context("Error getting current executable.")
  }

  fn log(&self, text: &str) {
    self.logger.log(text, "dprint");
  }

  fn log_machine_readable(&self, text: &str) {
    self.logger.log_machine_readable(text);
  }

  fn log_stderr_with_context(&self, text: &str, context_name: &str) {
    self.logger.log_stderr_with_context(text, context_name);
  }

  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult {
    log_action_with_progress(&self.progress_bars, message, action, total_size)
  }

  fn get_cache_dir(&self) -> CanonicalizedPathBuf {
    // ok to unwrap because this would have errored in the constructor
    (*CACHE_DIR.as_ref().unwrap()).clone()
  }

  fn cpu_arch(&self) -> String {
    std::env::consts::ARCH.to_string()
  }

  fn os(&self) -> String {
    let target = env!("TARGET");
    if target.contains("linux-musl") {
      "linux-musl".to_string()
    } else {
      std::env::consts::OS.to_string()
    }
  }

  fn max_threads(&self) -> usize {
    resolve_max_threads(std::env::var("DPRINT_MAX_THREADS").ok(), std::thread::available_parallelism().ok())
  }

  fn cli_version(&self) -> String {
    env!("CARGO_PKG_VERSION").to_string()
  }

  fn get_time_secs(&self) -> u64 {
    SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
  }

  fn get_selection(&self, prompt_message: &str, item_indent_width: u16, items: &[String]) -> Result<usize> {
    show_select(&self.logger, "dprint", prompt_message, item_indent_width, items)
  }

  fn get_multi_selection(&self, prompt_message: &str, item_indent_width: u16, items: &[(bool, String)]) -> Result<Vec<usize>> {
    show_multi_select(
      &self.logger,
      "dprint",
      prompt_message,
      item_indent_width,
      items.iter().map(|(value, text)| (value.to_owned(), text)).collect(),
    )
  }

  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool> {
    show_confirm(&self.logger, "dprint", prompt_message, default_value)
  }

  #[inline]
  fn is_verbose(&self) -> bool {
    self.logger.is_verbose()
  }

  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult> {
    crate::plugins::compile_wasm(wasm_bytes, self.clone())
  }

  fn stdout(&self) -> Box<dyn std::io::Write + Send> {
    Box::new(std::io::stdout())
  }

  fn stdin(&self) -> Box<dyn std::io::Read + Send> {
    Box::new(std::io::stdin())
  }

  fn runtime_handle(&self) -> tokio::runtime::Handle {
    (*self.runtime_handle).clone()
  }

  fn progress_bars(&self) -> Option<ProgressBars> {
    self.progress_bars.clone()
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    log_verbose!(self, "Ensuring '{}' is on the path.", directory_path);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = hkcu.create_subkey("Environment")?;
    let mut path: String = env.get_value("Path")?;

    // add to the path if it doesn't have this entry
    if !path.split(';').any(|p| p == directory_path) {
      if !path.is_empty() && !path.ends_with(';') {
        path.push(';')
      }
      path.push_str(directory_path);
      env.set_value("Path", &path)?;
    }
    Ok(())
  }

  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    log_verbose!(self, "Ensuring '{}' is on the path.", directory_path);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = hkcu.create_subkey("Environment")?;
    let path: String = env.get_value("Path")?;
    let mut paths = path.split(';').collect::<Vec<_>>();
    let original_len = paths.len();

    paths.retain(|p| p != &directory_path);

    let was_removed = original_len != paths.len();
    if was_removed {
      env.set_value("Path", &paths.join(";"))?;
    }
    Ok(())
  }
}

fn resolve_max_threads(env_var: Option<String>, available_parallelism: Option<NonZeroUsize>) -> usize {
  fn maybe_specified_threads(env_var: Option<String>) -> Option<usize> {
    let value = env_var?.parse::<usize>().ok()?;
    if value > 0 {
      Some(value)
    } else {
      None
    }
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

fn canonicalize_path(path: impl AsRef<Path>) -> Result<CanonicalizedPathBuf> {
  // use this to avoid //?//C:/etc... like paths on windows (UNC)
  match dunce::canonicalize(path.as_ref()) {
    Ok(result) => Ok(CanonicalizedPathBuf::new(result)),
    Err(err) => bail!("Error canonicalizing path {}: {:#}", path.as_ref().display(), err),
  }
}

const CACHE_DIR_ENV_VAR_NAME: &str = "DPRINT_CACHE_DIR";

static CACHE_DIR: Lazy<Result<CanonicalizedPathBuf>> = Lazy::new(|| {
  let cache_dir = get_cache_dir_internal(|var_name| std::env::var(var_name).ok())?;
  std::fs::create_dir_all(&cache_dir)?;
  canonicalize_path(cache_dir)
});

fn get_cache_dir_internal(get_env_var: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
  if let Some(dir_path) = get_env_var(CACHE_DIR_ENV_VAR_NAME) {
    if !dir_path.trim().is_empty() {
      let dir_path = PathBuf::from(dir_path);
      // seems dangerous to allow a relative path as this directory may be deleted
      return if !dir_path.is_absolute() {
        bail!("The {} environment variable must specify an absolute path.", CACHE_DIR_ENV_VAR_NAME)
      } else {
        Ok(dir_path)
      };
    }
  }

  match dirs::cache_dir() {
    Some(dir) => Ok(dir.join("dprint").join("cache")),
    None => bail!("Expected to find cache directory"),
  }
}

fn is_system_volume_error(dir_path: &Path, err: &std::io::Error) -> bool {
  // ignore any access denied errors for the system volume information
  cfg!(target_os = "windows")
    && matches!(err.raw_os_error(), Some(5))
    && matches!(dir_path.file_name().and_then(|f| f.to_str()), Some("System Volume Information"))
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

  #[test]
  fn should_resolve_num_threads() {
    assert_eq!(resolve_max_threads(None, None), 4);
    assert_eq!(resolve_max_threads(None, NonZeroUsize::new(1)), 1);
    assert_eq!(resolve_max_threads(None, NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("2".to_string()), NonZeroUsize::new(4)), 2);
    assert_eq!(resolve_max_threads(Some("0".to_string()), NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("5".to_string()), NonZeroUsize::new(4)), 4);
    assert_eq!(resolve_max_threads(Some("4".to_string()), NonZeroUsize::new(4)), 4);
  }
}
