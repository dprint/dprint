use anyhow::Result;
use anyhow::bail;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::hash::Hash;
use std::io;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::SystemTime;
use sys_traits::BaseEnvVar;
use sys_traits::BaseFsCreateDir;
use sys_traits::BaseFsMetadata;
use sys_traits::BaseFsOpen;
use sys_traits::BaseFsRead;
use sys_traits::BaseFsRemoveFile;
use sys_traits::BaseFsRename;
use sys_traits::BaseFsSetPermissions;
use sys_traits::CreateDirOptions;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;
use sys_traits::impls::RealSys;
use sysinfo::System;
use url::Url;

use dprint_core::async_runtime::async_trait;

use super::CanonicalizedPathBuf;
use super::DirEntry;
use super::DownloadOptions;
use super::DownloadedFile;
use super::Environment;
use super::FilePermissions;
use super::FilesWatcher;
use super::PathKind;
use super::UrlDownloader;
use crate::plugins::CompilationResult;
use crate::utils::DownloadPolicy;
use crate::utils::FastInsecureHasher;
use crate::utils::LogLevel;
use crate::utils::Logger;
use crate::utils::LoggerOptions;
use crate::utils::LoggerPromptTerminal;
use crate::utils::NoProxy;
use crate::utils::OutputSink;
use crate::utils::ProgressBars;
use crate::utils::ProgressBarsCell;
use crate::utils::PromptTerminal;
use crate::utils::RealUrlDownloader;
use crate::utils::ShowConfirmStrategy;
use crate::utils::UnsafelyIgnoreCertificates;
use crate::utils::is_terminal_interactive;
use crate::utils::log_action_with_progress;
use crate::utils::show_confirm;
use crate::utils::show_select;

// cache the cwd because it's much faster than looking it up each time
static CACHED_CWD: OnceCell<CanonicalizedPathBuf> = OnceCell::new();
// cache the global gitignore path because resolving it spawns a git subprocess
// and the path is stable for the process (used by the lsp and editor-service,
// which rebuild their file matchers on every config change)
static CACHED_GLOBAL_GITIGNORE_PATH: OnceCell<Option<PathBuf>> = OnceCell::new();
// cache the home dir because it's looked up for every `.config` directory
static CACHED_HOME_DIR: OnceCell<Option<CanonicalizedPathBuf>> = OnceCell::new();

pub struct RealEnvironmentOptions {
  pub log_level: LogLevel,
  pub is_stdout_machine_readable: bool,
}

#[derive(Clone)]
pub struct RealEnvironment {
  progress_bars: ProgressBarsCell,
  url_downloader: Arc<RealUrlDownloader>,
  logger: Arc<Logger>,
  system: Arc<Mutex<System>>,
  download_policy: Arc<Mutex<DownloadPolicy>>,
}

impl RealEnvironment {
  pub fn new(options: RealEnvironmentOptions) -> Result<RealEnvironment> {
    let logger = Arc::new(Logger::new(&LoggerOptions {
      initial_context_name: "dprint".to_string(),
      is_stdout_machine_readable: options.is_stdout_machine_readable,
      log_level: options.log_level,
    }));
    let progress_bars = ProgressBarsCell::new(ProgressBars::new(&logger).map(Arc::new));
    let no_proxy = NoProxy::from_env();
    let url_downloader = Arc::new(RealUrlDownloader::new(
      progress_bars.clone(),
      logger.clone(),
      no_proxy,
      UnsafelyIgnoreCertificates::from_env(),
    )?);
    let environment = RealEnvironment {
      url_downloader,
      logger,
      progress_bars,
      system: Default::default(),
      download_policy: Default::default(),
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

  /// Expands a leading `~` to the user's home directory, mirroring how git
  /// expands `core.excludesFile`.
  fn expand_user_path(&self, value: &str) -> Option<PathBuf> {
    if value == "~" {
      Some(self.get_home_dir()?.into_path_buf())
    } else if let Some(rest) = value.strip_prefix("~/") {
      Some(self.get_home_dir()?.join(rest))
    } else {
      Some(PathBuf::from(value))
    }
  }

  /// Resolves git's global excludes file path. Prefer the cached
  /// `global_gitignore_path` over calling this directly, as this spawns a git
  /// subprocess.
  fn resolve_global_gitignore_path(&self) -> Option<PathBuf> {
    // prefer the path configured via `git config core.excludesFile`
    let configured = Command::new("git")
      .arg("config")
      .arg("--get")
      .arg("core.excludesFile")
      .output()
      .ok()
      .filter(|output| output.status.success())
      .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
      .filter(|value| !value.is_empty());

    match configured {
      Some(value) => self.expand_user_path(&value),
      None => {
        // git's default location when `core.excludesFile` is unset
        let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
          Some(xdg) => PathBuf::from(xdg),
          None => self.get_home_dir()?.join(".config"),
        };
        Some(base.join("git").join("ignore"))
      }
    }
  }

  #[cfg(test)]
  pub fn run_test_with_real_env(run_with_env: impl Fn(RealEnvironment) -> dprint_core::async_runtime::LocalBoxFuture<'static, ()>) {
    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .enable_time()
      .thread_stack_size(crate::plugins::WASM_PLUGIN_THREAD_STACK_SIZE)
      .build()
      .unwrap();
    let env = RealEnvironment::new(RealEnvironmentOptions {
      log_level: LogLevel::Info,
      is_stdout_machine_readable: false,
    })
    .unwrap();

    rt.block_on(async move { run_with_env(env).await });
  }
}

impl std::fmt::Debug for RealEnvironment {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RealEnvironment").finish()
  }
}

impl BaseFsCreateDir for RealEnvironment {
  fn base_fs_create_dir(&self, path: &Path, options: &CreateDirOptions) -> io::Result<()> {
    RealSys.base_fs_create_dir(path, options)
  }
}

impl BaseEnvVar for RealEnvironment {
  fn base_env_var_os(&self, key: &OsStr) -> Option<OsString> {
    RealSys.base_env_var_os(key)
  }
}

impl BaseFsMetadata for RealEnvironment {
  type Metadata = sys_traits::impls::RealFsMetadata;

  fn base_fs_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
    RealSys.base_fs_metadata(path)
  }

  fn base_fs_symlink_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
    RealSys.base_fs_symlink_metadata(path)
  }
}

impl BaseFsOpen for RealEnvironment {
  type File = sys_traits::impls::RealFsFile;

  fn base_fs_open(&self, path: &Path, options: &sys_traits::OpenOptions) -> io::Result<Self::File> {
    RealSys.base_fs_open(path, options)
  }
}

impl BaseFsRead for RealEnvironment {
  fn base_fs_read(&self, path: &Path) -> io::Result<Cow<'static, [u8]>> {
    RealSys.base_fs_read(path)
  }
}

impl BaseFsRemoveFile for RealEnvironment {
  fn base_fs_remove_file(&self, path: &Path) -> io::Result<()> {
    RealSys.base_fs_remove_file(path)
  }
}

impl BaseFsRename for RealEnvironment {
  fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
    RealSys.base_fs_rename(from, to)
  }
}

impl BaseFsSetPermissions for RealEnvironment {
  fn base_fs_set_permissions(&self, path: &Path, mode: u32) -> io::Result<()> {
    RealSys.base_fs_set_permissions(path, mode)
  }
}

impl ThreadSleep for RealEnvironment {
  fn thread_sleep(&self, duration: std::time::Duration) {
    std::thread::sleep(duration);
  }
}

impl SystemRandom for RealEnvironment {
  fn sys_random(&self, buf: &mut [u8]) -> io::Result<()> {
    use rand::RngCore;
    rand::rng().fill_bytes(buf);
    Ok(())
  }
}

impl SystemTimeNow for RealEnvironment {
  fn sys_time_now(&self) -> SystemTime {
    SystemTime::now()
  }
}

#[async_trait(?Send)]
impl UrlDownloader for RealEnvironment {
  async fn download_file_no_redirects(&self, url: &Url, options: DownloadOptions<'_>) -> Result<Option<DownloadedFile>> {
    self.download_policy.lock().ensure_allows_download(url, options.proxy)?;
    log_debug!(self, "Downloading url: {}", url);

    self.url_downloader.download(url, options).await
  }
}

#[async_trait]
impl Environment for RealEnvironment {
  fn is_real(&self) -> bool {
    true
  }

  fn env_var(&self, name: &str) -> Option<OsString> {
    std::env::var_os(name)
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = self.read_file_bytes(file_path.as_ref())?;
    String::from_utf8(bytes).map_err(|err| {
      io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Error converting file to utf8 {}: {:#}", file_path.as_ref().display(), err),
      )
    })
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    log_debug!(self, "Reading file: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    match fs::read(&file_path) {
      Ok(bytes) => Ok(bytes),
      Err(err) => Err(io::Error::new(
        err.kind(),
        format!("Error reading file {}: {:#}", file_path.as_ref().display(), err),
      )),
    }
  }

  fn get_staged_files(&self) -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
      .arg("diff")
      .arg("--name-only")
      .arg("--relative")
      .arg("--staged")
      .arg("--diff-filter=ACMR")
      .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).lines().map(PathBuf::from).collect())
  }

  fn get_dirty_files(&self) -> Result<Vec<PathBuf>> {
    // collect every file with uncommitted changes in the working directory:
    // unstaged tracked changes, staged tracked changes, and untracked files
    // that aren't gitignored. each is gathered the same way `get_staged_files`
    // gathers staged files so the behaviour (e.g. renamed files yielding their
    // new path, deletions being skipped) stays consistent.
    fn git_lines(args: &[&str]) -> Result<Vec<PathBuf>> {
      let output = Command::new("git").args(args).output()?;
      Ok(String::from_utf8_lossy(&output.stdout).lines().map(PathBuf::from).collect())
    }

    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let groups = [
      // unstaged tracked changes
      git_lines(&["diff", "--name-only", "--relative", "--diff-filter=ACMR"])?,
      // staged tracked changes
      git_lines(&["diff", "--name-only", "--relative", "--staged", "--diff-filter=ACMR"])?,
      // untracked files that aren't gitignored
      git_lines(&["ls-files", "--others", "--exclude-standard"])?,
    ];
    for file in groups.into_iter().flatten() {
      if seen.insert(file.clone()) {
        files.push(file);
      }
    }
    Ok(files)
  }

  fn global_gitignore_path(&self) -> Option<PathBuf> {
    CACHED_GLOBAL_GITIGNORE_PATH.get_or_init(|| self.resolve_global_gitignore_path()).clone()
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    log_debug!(self, "Writing file: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    match fs::write(&file_path, bytes) {
      Ok(_) => Ok(()),
      Err(err) => Err(io::Error::new(
        err.kind(),
        format!("Error writing file '{}': {:#}", file_path.as_ref().display(), err),
      )),
    }
  }

  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> io::Result<()> {
    log_debug!(self, "Renaming {} -> {}", path_from.as_ref().display(), path_to.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    fs::rename(&path_from, &path_to).map_err(|err| {
      io::Error::new(
        err.kind(),
        format!(
          "Error renaming '{}' to '{}': {:#}",
          path_from.as_ref().display(),
          path_to.as_ref().display(),
          err
        ),
      )
    })
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> io::Result<()> {
    log_debug!(self, "Deleting file: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    match fs::remove_file(&file_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
      Err(err) => Err(io::Error::new(
        err.kind(),
        format!("Error deleting file '{}': {:#}", file_path.as_ref().display(), err),
      )),
    }
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> io::Result<()> {
    log_debug!(self, "Deleting directory: {}", dir_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    match fs::remove_dir_all(&dir_path) {
      Ok(_) => Ok(()),
      Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
      Err(err) => Err(io::Error::new(
        err.kind(),
        format!("Error deleting directory '{}': {:#}", dir_path.as_ref().display(), err),
      )),
    }
  }

  fn kill_processes_using_dir(&self, dir_path: impl AsRef<Path>) -> usize {
    let dir_path = dir_path.as_ref();
    let mut system = self.system.lock();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut killed_pids = Vec::new();
    for process in system.processes().values() {
      let Some(exe) = process.exe() else {
        continue;
      };
      if exe.starts_with(dir_path) {
        log_debug!(self, "Killing process {} using executable: {}", process.pid(), exe.display());
        if process.kill() {
          killed_pids.push(process.pid());
        }
      }
    }

    // wait for the killed processes to actually exit so their executables are no
    // longer locked before the caller tries to delete them again. poll with a
    // timeout rather than `Process::wait`, which blocks indefinitely (e.g. on a
    // process we couldn't kill, or a zombie its real parent hasn't reaped yet).
    if !killed_pids.is_empty() {
      let mut remaining_polls = 100; // ~2s at 20ms per poll
      while remaining_polls > 0 {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&killed_pids), true);
        if killed_pids.iter().all(|pid| system.process(*pid).is_none()) {
          break;
        }
        remaining_polls -= 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
      }
    }

    killed_pids.len()
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();

    #[allow(clippy::disallowed_methods)]
    let dir_info = std::fs::read_dir(&dir_path)?;

    for entry in dir_info {
      let entry = entry?;
      let file_type = entry.file_type()?;
      if file_type.is_dir() {
        entries.push(DirEntry::Directory(entry.path()));
      } else if file_type.is_file() {
        entries.push(DirEntry::File { path: entry.path() });
      }
    }

    Ok(entries)
  }

  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool {
    log_debug!(self, "Checking path exists: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    file_path.as_ref().exists()
  }

  fn path_is_file(&self, file_path: impl AsRef<Path>) -> bool {
    log_debug!(self, "Checking path is file: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    file_path.as_ref().is_file()
  }

  fn path_kind(&self, file_path: impl AsRef<Path>) -> Option<PathKind> {
    log_debug!(self, "Statting path: {}", file_path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    let file_type = file_path.as_ref().symlink_metadata().ok()?.file_type();
    Some(if file_type.is_symlink() {
      PathKind::Symlink
    } else if file_type.is_dir() {
      PathKind::Dir
    } else {
      PathKind::File
    })
  }

  fn read_link(&self, path: impl AsRef<Path>) -> io::Result<PathBuf> {
    log_debug!(self, "Reading symlink: {}", path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    fs::read_link(path)
  }

  fn is_automount_dir(&self, dir_path: impl AsRef<Path>) -> bool {
    log_debug!(self, "Getting the file system of: {}", dir_path.as_ref().display());
    is_automount_dir(dir_path.as_ref())
  }

  fn is_automount_point(&self, path: impl AsRef<Path>) -> bool {
    log_debug!(self, "Getting whether a share is mounted at: {}", path.as_ref().display());
    is_automount_point(path.as_ref())
  }

  fn canonicalize(&self, path: impl AsRef<Path>) -> io::Result<CanonicalizedPathBuf> {
    canonicalize_path(path)
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    path.as_ref().is_absolute()
  }

  fn file_permissions(&self, path: impl AsRef<Path>) -> io::Result<FilePermissions> {
    Ok(FilePermissions::Std(
      #[allow(clippy::disallowed_methods)]
      fs::metadata(&path)
        .map_err(|err| {
          io::Error::new(
            err.kind(),
            format!("Error getting file permissions for '{}': {:#}", path.as_ref().display(), err),
          )
        })?
        .permissions(),
    ))
  }

  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> io::Result<()> {
    let permissions = match permissions {
      FilePermissions::Std(p) => p,
      _ => panic!("Programming error. Permissions did not contain an std permission."),
    };
    #[allow(clippy::disallowed_methods)]
    fs::set_permissions(&path, permissions).map_err(|err| {
      io::Error::new(
        err.kind(),
        format!("Error setting file permissions for '{}': {:#}", path.as_ref().display(), err),
      )
    })?;
    Ok(())
  }

  fn mk_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
    log_debug!(self, "Creating directory: {}", path.as_ref().display());
    #[allow(clippy::disallowed_methods)]
    match fs::create_dir_all(&path) {
      Ok(_) => Ok(()),
      Err(err) => Err(io::Error::new(
        err.kind(),
        format!("Error creating directory '{}': {:#}", path.as_ref().display(), err),
      )),
    }
  }

  fn cwd(&self) -> CanonicalizedPathBuf {
    CACHED_CWD
      .get_or_init(|| {
        #[allow(clippy::disallowed_methods)]
        self
          .canonicalize(std::env::current_dir().expect("Expected to get the current working directory."))
          .expect("expected to canonicalize the cwd")
      })
      .clone()
  }

  fn current_exe(&self) -> io::Result<PathBuf> {
    std::env::current_exe().map_err(|err| io::Error::new(err.kind(), format!("Error getting current executable: {:#}", err)))
  }

  fn __log__(&self, text: &str) {
    self.logger.log(text, "dprint");
  }

  fn log_machine_readable(&self, bytes: &[u8]) {
    self.logger.log_machine_readable(bytes);
  }

  fn log_stderr_with_context(&self, level: LogLevel, text: &str, context_name: &str) {
    self.logger.log_stderr_with_context(level, text, context_name);
  }

  fn set_output_sink(&self, sink: Arc<dyn OutputSink>) {
    self.logger.set_sink(sink.clone());
    self.progress_bars.set(Some(Arc::new(ProgressBars::with_sink(sink))));
  }

  fn download_policy(&self) -> DownloadPolicy {
    self.download_policy.lock().clone()
  }

  fn set_download_policy(&self, policy: DownloadPolicy) {
    *self.download_policy.lock() = policy;
  }

  fn watch_files(&self, file_paths: &[PathBuf]) -> io::Result<FilesWatcher> {
    use notify::Watcher;

    let (sender, changes) = tokio::sync::mpsc::unbounded_channel();
    let file_names = file_paths
      .iter()
      .filter_map(|path| path.file_name().map(|name| name.to_os_string()))
      .collect::<HashSet<_>>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
      // the directories are watched, so only their files with these names are changes
      if let Ok(event) = result
        && event.paths.iter().any(|path| path.file_name().is_some_and(|name| file_names.contains(name)))
      {
        let _ = sender.send(());
      }
    })
    .map_err(io::Error::other)?;
    // a file may not exist yet (ex. it's created later), so its directory is watched
    let mut dir_paths = Vec::new();
    for dir_path in file_paths.iter().filter_map(|path| path.parent()) {
      if !dir_paths.contains(&dir_path) {
        dir_paths.push(dir_path);
      }
    }
    for dir_path in dir_paths {
      watcher.watch(dir_path, notify::RecursiveMode::NonRecursive).map_err(io::Error::other)?;
    }
    Ok(FilesWatcher::new(changes, watcher))
  }

  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult {
    log_action_with_progress(self.progress_bars.get().as_deref(), message, action, total_size)
  }

  fn get_cache_dir(&self) -> CanonicalizedPathBuf {
    // ok to unwrap because this would have errored in the constructor
    (*CACHE_DIR.as_ref().unwrap()).clone()
  }

  fn get_config_dir(&self) -> Option<PathBuf> {
    dirs::config_dir()
  }

  fn get_home_dir(&self) -> Option<CanonicalizedPathBuf> {
    CACHED_HOME_DIR
      // the home directory might not exist (ex. `HOME=/nonexistent` in a container)
      .get_or_init(|| dirs::home_dir().and_then(|path| self.canonicalize(path).ok()))
      .clone()
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

  fn available_parallelism(&self) -> Option<NonZeroUsize> {
    std::thread::available_parallelism().ok()
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

  fn prompt_terminal(&self) -> Box<dyn PromptTerminal + '_> {
    Box::new(LoggerPromptTerminal::new(&self.logger))
  }

  fn confirm_with_strategy(&self, strategy: &dyn ShowConfirmStrategy) -> Result<bool> {
    show_confirm(&self.logger, "dprint", strategy)
  }

  fn run_command_get_status(&self, mut args: Vec<OsString>) -> io::Result<Option<i32>> {
    let command_name = args.remove(0);
    let command_path = which::which(command_name).map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
    std::process::Command::new(command_path).args(args).status().map(|s| s.code())
  }

  fn is_ci(&self) -> bool {
    match std::env::var_os("CI") {
      Some(value) => {
        let value = value.to_string_lossy();
        matches!(value.as_ref(), "true" | "1")
      }
      None => false,
    }
  }

  fn is_terminal_interactive(&self) -> bool {
    is_terminal_interactive()
  }

  #[inline]
  fn log_level(&self) -> LogLevel {
    self.logger.log_level()
  }

  fn set_log_level(&self, log_level: LogLevel) {
    self.logger.set_log_level(log_level);
  }

  fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult> {
    crate::plugins::compile_wasm(wasm_bytes, self.max_threads())
  }

  fn wasm_cache_key(&self) -> String {
    let cpu = self.cpu_arch();
    let mut hash = FastInsecureHasher::default();
    // wasmtime tunes native code to the host CPU features and refuses to
    // deserialize an artifact compiled for incompatible ones (the caller then
    // recompiles), so include what it checks in the key. that way artifacts for
    // different CPUs get distinct cache entries and coexist in a cache directory
    // shared across machines, such as one restored from a CI cache, rather than
    // each machine overwriting the other's. the rustc version is hashed too
    // because deserialization can break across a rust upgrade.
    // https://github.com/dprint/dprint/issues/735
    env!("RUSTC_VERSION_TEXT").hash(&mut hash);
    crate::plugins::wasm_precompile_compatibility_hash().hash(&mut hash);
    format!("{}-{}", cpu, hash.finish())
  }

  async fn cpu_usage(&self) -> u8 {
    // the documentation recommends calling this twice in order
    // to get a more accurate cpu reading
    let system = self.system.clone();
    let Ok(system) = dprint_core::async_runtime::spawn_blocking(move || {
      {
        let mut system = system.lock();
        system.refresh_cpu_usage();
      }
      system
    })
    .await
    else {
      return 0;
    };

    // wait a duration that allows getting a more accurate cpu usage
    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;

    dprint_core::async_runtime::spawn_blocking(move || {
      let mut system = system.lock();
      system.refresh_cpu_usage();
      let utilization = system.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / system.cpus().len() as f32;
      if utilization > 101f32 {
        0 // something wrong, so just return 0 for "cannot figure out cpu usage"
      } else {
        utilization as u8
      }
    })
    .await
    .unwrap_or(0)
  }

  fn stdout(&self) -> Box<dyn io::Write + Send> {
    Box::new(io::stdout())
  }

  fn stdin(&self) -> Box<dyn io::Read + Send> {
    Box::new(io::stdin())
  }

  fn progress_bars(&self) -> Option<Arc<ProgressBars>> {
    self.progress_bars.get()
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;
    log_debug!(self, "Ensuring '{}' is on the path.", directory_path);

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
  fn remove_system_path(&self, directory_path: &str) -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;
    log_debug!(self, "Ensuring '{}' is on the path.", directory_path);

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

fn canonicalize_path(path: impl AsRef<Path>) -> io::Result<CanonicalizedPathBuf> {
  // use this to avoid //?//C:/etc... like paths on windows (UNC)
  match dunce::canonicalize(path.as_ref()) {
    Ok(result) => Ok(CanonicalizedPathBuf::new(result)),
    Err(err) => Err(io::Error::new(
      err.kind(),
      format!("Error canonicalizing path '{}': {:#}", path.as_ref().display(), err),
    )),
  }
}

const CACHE_DIR_ENV_VAR_NAME: &str = "DPRINT_CACHE_DIR";

static CACHE_DIR: Lazy<io::Result<CanonicalizedPathBuf>> = Lazy::new(|| {
  #[allow(clippy::disallowed_methods)]
  let cache_dir = get_cache_dir_internal(|var_name| std::env::var(var_name).ok())?;
  #[allow(clippy::disallowed_methods)]
  std::fs::create_dir_all(&cache_dir)?;
  canonicalize_path(cache_dir)
});

fn get_cache_dir_internal(get_env_var: impl Fn(&str) -> Option<String>) -> io::Result<PathBuf> {
  if let Some(dir_path) = get_env_var(CACHE_DIR_ENV_VAR_NAME)
    && !dir_path.trim().is_empty()
  {
    let dir_path = PathBuf::from(dir_path);
    // seems dangerous to allow a relative path as this directory may be deleted
    return if !dir_path.is_absolute() {
      Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("The {} environment variable must specify an absolute path.", CACHE_DIR_ENV_VAR_NAME),
      ))
    } else {
      Ok(dir_path)
    };
  }

  match dirs::cache_dir() {
    Some(dir) => Ok(dir.join("dprint").join("cache")),
    None => Err(io::Error::new(io::ErrorKind::NotFound, "Expected to find cache directory.")),
  }
}

/// Gets whether the directory is the root of an automounter's file system
/// (see `Environment::is_automount_dir`) from the type of its file system.
///
/// IMPORTANT: Linux's `statfs` mounts what an automounter mounts at the
/// directory (ex. the share of a direct map), so the type is gotten from a
/// descriptor that `O_PATH` opens, which doesn't mount it, and that doesn't
/// look up the directory's children either.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn is_automount_dir(dir_path: &Path) -> bool {
  use std::os::fd::AsRawFd;
  use std::os::fd::FromRawFd;
  use std::os::fd::OwnedFd;
  use std::os::unix::ffi::OsStrExt;

  let Ok(dir_path) = std::ffi::CString::new(dir_path.as_os_str().as_bytes()) else {
    return false;
  };
  // SAFETY: the path is nul terminated
  let fd = unsafe { libc::open(dir_path.as_ptr(), libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
  if fd < 0 {
    return false;
  }
  // SAFETY: the descriptor was just opened and nothing else owns it
  let fd = unsafe { OwnedFd::from_raw_fd(fd) };
  let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
  // SAFETY: the descriptor is open and the buffer has the size of what's written to it
  if unsafe { libc::fstatfs(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
    return false;
  }
  // SAFETY: it succeeded, so it was written
  let stat = unsafe { stat.assume_init() };
  // the type's size and sign differ between the targets
  #[allow(clippy::unnecessary_cast)]
  let is_autofs = stat.f_type as u64 == AUTOFS_SUPER_MAGIC;
  is_autofs
}

/// Gets whether the directory is the root of an automounter's file system
/// (see `Environment::is_automount_dir`) from the type of its file system,
/// which `statfs` gets without looking up the directory's children.
#[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
fn is_automount_dir(dir_path: &Path) -> bool {
  use std::os::unix::ffi::OsStrExt;

  let Ok(dir_path) = std::ffi::CString::new(dir_path.as_os_str().as_bytes()) else {
    return false;
  };
  let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
  // SAFETY: the path is nul terminated and the buffer has the size of what's written to it
  if unsafe { libc::statfs(dir_path.as_ptr(), stat.as_mut_ptr()) } != 0 {
    return false;
  }
  // SAFETY: it succeeded, so it was written
  let stat = unsafe { stat.assume_init() };
  // SAFETY: the name is nul terminated
  let type_name = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()) };
  type_name.to_bytes() == b"autofs"
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple", target_os = "freebsd")))]
fn is_automount_dir(_dir_path: &Path) -> bool {
  false
}

/// Gets whether a share is mounted at the path when it's looked up (see
/// `Environment::is_automount_point`) from `statx`, which doesn't mount it
/// with `AT_NO_AUTOMOUNT`.
///
/// It's called with `syscall` because the C libraries that dprint is built
/// with may not have it (musl, and glibc before 2.28), and it's `false` when
/// the kernel doesn't have it either (before Linux 4.11).
#[cfg(target_os = "linux")]
fn is_automount_point(path: &Path) -> bool {
  use std::os::unix::ffi::OsStrExt;

  let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
    return false;
  };
  let mut stat = Statx {
    _mask_and_block_size: [0; 2],
    stx_attributes: 0,
    _rest: [0; 30],
  };
  // SAFETY: the path is nul terminated and the buffer has the size of what the kernel writes to it
  let result = unsafe {
    libc::syscall(
      libc::SYS_statx,
      libc::AT_FDCWD,
      path.as_ptr(),
      libc::AT_SYMLINK_NOFOLLOW | libc::AT_NO_AUTOMOUNT,
      STATX_TYPE,
      &mut stat as *mut Statx,
    )
  };
  result == 0 && (stat.stx_attributes & STATX_ATTR_AUTOMOUNT) != 0
}

#[cfg(not(target_os = "linux"))]
fn is_automount_point(_path: &Path) -> bool {
  false
}

/// The `f_type` of an `autofs` file system on Linux.
#[cfg(any(target_os = "linux", target_os = "android"))]
const AUTOFS_SUPER_MAGIC: u64 = 0x0187;

/// What `statx` is asked for, which is only the type because the attributes
/// are always gotten. The `libc` crate only has it and the attribute below
/// for some C libraries (ex. not musl).
#[cfg(target_os = "linux")]
const STATX_TYPE: libc::c_uint = 0x0001;

/// The attribute of `statx` for an automount point.
#[cfg(target_os = "linux")]
const STATX_ATTR_AUTOMOUNT: u64 = 0x1000;

/// The `statx` struct of Linux, which is 256 bytes on every architecture,
/// where only the attributes are read.
#[cfg(target_os = "linux")]
#[repr(C)]
struct Statx {
  /// `stx_mask` and `stx_blksize`.
  _mask_and_block_size: [u32; 2],
  stx_attributes: u64,
  _rest: [u64; 30],
}

#[cfg(target_os = "linux")]
const _: () = assert!(std::mem::size_of::<Statx>() == 256);

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_watch_files_that_may_not_exist() {
    use dprint_core::async_runtime::FutureExt;

    RealEnvironment::run_test_with_real_env(|environment| {
      async move {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("settings.json");
        let mut watcher = environment.watch_files(std::slice::from_ref(&file_path)).unwrap();
        // another file in the directory isn't a change
        environment.write_file(temp_dir.path().join("other.json"), "{}").unwrap();
        environment.write_file(&file_path, "{}").unwrap();
        let change = tokio::time::timeout(std::time::Duration::from_secs(10), watcher.changes.recv()).await;
        assert!(matches!(change, Ok(Some(()))));
        // a directory that doesn't exist can't be watched
        assert!(environment.watch_files(&[temp_dir.path().join("missing").join("settings.json")]).is_err());
      }
      .boxed_local()
    });
  }

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
