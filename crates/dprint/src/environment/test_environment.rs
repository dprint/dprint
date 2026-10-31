use anyhow::Result;
use anyhow::anyhow;
use once_cell::sync::Lazy;
use parking_lot::Condvar;
use parking_lot::Mutex;
use path_clean::PathClean;
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::io::Read;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use sys_traits::BaseEnvVar;
use sys_traits::BaseFsCreateDir;
use sys_traits::BaseFsMetadata;
use sys_traits::BaseFsOpen;
use sys_traits::BaseFsRead;
use sys_traits::BaseFsRemoveFile;
use sys_traits::BaseFsRename;
use sys_traits::BaseFsSetPermissions;
use sys_traits::CreateDirOptions;
use sys_traits::EnvCurrentDir;
use sys_traits::EnvRemoveVar;
use sys_traits::EnvSetCurrentDir;
use sys_traits::EnvSetVar;
use sys_traits::EnvVar;
use sys_traits::FsCreateDirAll;
use sys_traits::FsDirEntry;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;
use sys_traits::FsRead;
use sys_traits::FsReadDir;
use sys_traits::FsReadLink;
use sys_traits::FsRemoveDirAll;
use sys_traits::FsRemoveFile;
use sys_traits::FsRename;
use sys_traits::FsSetPermissions;
use sys_traits::FsSymlinkFile;
use sys_traits::FsWrite;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;
use sys_traits::impls::InMemorySys;
use url::Url;

use dprint_core::async_runtime::async_trait;

use super::CanonicalizedPathBuf;
use super::DirEntry;
use super::DownloadOptions;
use super::DownloadedFile;
use super::Environment;
use super::FileLock;
use super::FilePermissions;
use super::FilesWatcher;
use super::PathKind;
use super::UrlDownloader;
use crate::plugins::CompilationResult;
use crate::utils::DownloadPolicy;
use crate::utils::LogLevel;
use crate::utils::OutputSink;
use crate::utils::ProgressBars;
use crate::utils::ProgressBarsCell;
use crate::utils::PromptScreen;
use crate::utils::PromptTerminal;
use crate::utils::ScriptedPromptTerminal;
use crate::utils::ShowConfirmStrategy;
use crate::utils::TerminalSize;
use crate::utils::get_bytes_hash;
use crate::utils::get_sink_text;
use crate::utils::get_stderr_text;
use crate::utils::log_action_with_progress;

#[derive(Default)]
struct BufferData {
  data: Vec<u8>,
  read_pos: usize,
  closed: bool,
}

#[derive(Default)]
struct PipeData {
  cond_var: Condvar,
  buffer_data: Mutex<BufferData>,
}

fn create_test_pipe() -> (TestPipeWriter, TestPipeReader) {
  let buffer_data = Arc::new(PipeData::default());
  (TestPipeWriter(buffer_data.clone()), TestPipeReader(buffer_data))
}

#[derive(Clone)]
struct TestPipeReader(Arc<PipeData>);

// Prevent having to deal with deadlock headaches by
// only allowing one of these to be created
struct TestPipeWriter(Arc<PipeData>);

impl Drop for TestPipeWriter {
  fn drop(&mut self) {
    self.0.buffer_data.lock().closed = true;
    self.0.cond_var.notify_one();
  }
}

impl Read for TestPipeReader {
  fn read(&mut self, _: &mut [u8]) -> Result<usize, io::Error> {
    panic!("Not implemented");
  }

  fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), io::Error> {
    let mut buffer_data = self.0.buffer_data.lock();

    while buffer_data.data.len() < buffer_data.read_pos + buf.len() && !buffer_data.closed {
      self.0.cond_var.wait(&mut buffer_data);
    }

    if buffer_data.data.len() == buffer_data.read_pos && buffer_data.closed {
      return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe."));
    }

    buf.copy_from_slice(&buffer_data.data[buffer_data.read_pos..buffer_data.read_pos + buf.len()]);
    buffer_data.read_pos += buf.len();

    Ok(())
  }
}

impl Write for TestPipeWriter {
  fn write(&mut self, data: &[u8]) -> Result<usize, io::Error> {
    let result = {
      let mut buffer_data = self.0.buffer_data.lock();
      buffer_data.data.write(data)
    };
    self.0.cond_var.notify_one();
    result
  }

  fn flush(&mut self) -> Result<(), io::Error> {
    Ok(())
  }
}

#[derive(Clone)]
pub struct TestEnvironment {
  log_level: Arc<Mutex<LogLevel>>,
  download_policy: Arc<Mutex<DownloadPolicy>>,
  /// Where to log and show progress instead of stderr.
  output_sink: Arc<Mutex<Option<Arc<dyn OutputSink>>>>,
  progress_bars: ProgressBarsCell,
  sys: Arc<InMemorySys>,
  staged_files: Arc<Mutex<Vec<PathBuf>>>,
  dirty_files: Arc<Mutex<Vec<PathBuf>>>,
  global_gitignore_path: Arc<Mutex<Option<PathBuf>>>,
  stdout_messages: Arc<Mutex<Vec<String>>>,
  stderr_messages: Arc<Mutex<Vec<String>>>,
  remote_files: Arc<Mutex<HashMap<String, Result<Vec<u8>>>>>,
  /// Response headers of the remote files that have them.
  remote_file_headers: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
  remote_file_redirects: Arc<Mutex<HashMap<String, String>>>,
  /// Last auth header seen for each URL.
  remote_file_auth: Arc<Mutex<HashMap<String, Option<String>>>>,
  /// Last `DownloadOptions` seen for each URL, rendered for assertions.
  remote_file_options: Arc<Mutex<HashMap<String, String>>>,
  remote_file_proxies: Arc<Mutex<HashMap<String, String>>>,
  /// Number of times each URL was requested.
  remote_file_request_counts: Arc<Mutex<HashMap<String, usize>>>,
  /// Number of times each file was read (whether or not it existed).
  file_read_counts: Arc<Mutex<HashMap<PathBuf, usize>>>,
  /// Number of times each path was canonicalized, which follows its symlinks.
  canonicalize_counts: Arc<Mutex<HashMap<PathBuf, usize>>>,
  /// The paths that are canonicalized to another one, which is like a
  /// symlink to a local file or directory (see `add_canonical_symlink`).
  canonical_paths: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
  /// Number of times each file was opened (ex. to read its shebang line).
  file_open_counts: Arc<Mutex<HashMap<PathBuf, usize>>>,
  /// Number of times each path was looked up following its symlinks (ex.
  /// `path_exists` or `fs_metadata`).
  stat_counts: Arc<Mutex<HashMap<PathBuf, usize>>>,
  /// How many times the kind of a path was gotten (see `Environment::path_kind`).
  path_kind_count: Arc<Mutex<usize>>,
  /// The directories that are the roots of an automounter's file system (see
  /// `Environment::is_automount_dir`).
  automount_dirs: Arc<Mutex<HashSet<PathBuf>>>,
  /// The paths where a share is mounted from another machine when they're
  /// looked up (see `Environment::is_automount_point`).
  automount_points: Arc<Mutex<HashSet<PathBuf>>>,
  /// URLs whose requests never get a response.
  unresponsive_remote_files: Arc<Mutex<Vec<String>>>,
  selection_result: Arc<Mutex<usize>>,
  /// The keys to press in the multi-selection prompts and settings editors,
  /// which are then really shown instead of being answered with the result
  /// above.
  prompt_terminal: Arc<Mutex<Option<ScriptedPromptTerminal>>>,
  /// The items of the last multi-selection prompt, rendered for assertions.
  confirm_results: Arc<Mutex<Vec<Result<Option<bool>>>>>,
  is_stdout_machine_readable: Arc<Mutex<bool>>,
  dir_info_error: Arc<Mutex<Option<io::Error>>>,
  std_in_pipe: Arc<Mutex<(Option<TestPipeWriter>, TestPipeReader)>>,
  std_out_pipe: Arc<Mutex<(Option<TestPipeWriter>, TestPipeReader)>>,
  #[cfg(windows)]
  path_dirs: Arc<Mutex<Vec<PathBuf>>>,
  cpu_arch: Arc<Mutex<String>>,
  os: Arc<Mutex<String>>,
  max_threads_count: Arc<Mutex<usize>>,
  current_exe_path: Arc<Mutex<PathBuf>>,
  is_terminal_interactive: Arc<Mutex<bool>>,
  run_command_results: Arc<Mutex<Vec<(Vec<OsString>, io::Result<Option<i32>>)>>>,
  /// Executables of processes that are pretending to be running, each paired
  /// with the number of times it will "restart" (re-lock its directory) after
  /// being killed. A directory containing one of these can't be removed
  /// (simulating a locked executable) until the process stays killed via
  /// `kill_processes_using_dir`. The restart count models an editor such as the
  /// VSCode extension respawning its process plugins.
  running_processes: Arc<Mutex<Vec<(PathBuf, usize)>>>,
  /// Number of times `remove_dir_all` should fail with a transient error before
  /// succeeding, independent of any running process. Models a flaky deletion
  /// (e.g. a file briefly locked) that succeeds on retry.
  remove_dir_all_failures: Arc<Mutex<usize>>,
  /// The files that are watched with what's told when one changes.
  file_watchers: Arc<Mutex<Vec<(Vec<PathBuf>, tokio::sync::mpsc::UnboundedSender<()>)>>>,
  /// The files that are locked (see `Environment::try_lock_file`).
  locked_files: Arc<Mutex<HashSet<PathBuf>>>,
  /// The files that fail to be written (ex. the disk is full).
  failing_file_writes: Arc<Mutex<HashSet<PathBuf>>>,
  /// The files that fail to be deleted (ex. another process has them open).
  failing_file_removes: Arc<Mutex<HashSet<PathBuf>>>,
  /// The files that fail to be locked (ex. the file system doesn't support it).
  failing_file_locks: Arc<Mutex<HashSet<PathBuf>>>,
}

impl TestEnvironment {
  pub fn new() -> TestEnvironment {
    let env = TestEnvironment {
      log_level: Arc::new(Mutex::new(LogLevel::Info)),
      download_policy: Default::default(),
      output_sink: Default::default(),
      progress_bars: Default::default(),
      sys: Default::default(),
      staged_files: Default::default(),
      dirty_files: Default::default(),
      global_gitignore_path: Default::default(),
      stdout_messages: Default::default(),
      stderr_messages: Default::default(),
      remote_files: Default::default(),
      remote_file_headers: Default::default(),
      remote_file_redirects: Default::default(),
      remote_file_auth: Default::default(),
      remote_file_options: Default::default(),
      remote_file_proxies: Default::default(),
      remote_file_request_counts: Default::default(),
      file_read_counts: Default::default(),
      canonicalize_counts: Default::default(),
      canonical_paths: Default::default(),
      file_open_counts: Default::default(),
      stat_counts: Default::default(),
      path_kind_count: Default::default(),
      automount_dirs: Default::default(),
      automount_points: Default::default(),
      unresponsive_remote_files: Default::default(),
      selection_result: Arc::new(Mutex::new(0)),
      prompt_terminal: Default::default(),
      confirm_results: Default::default(),
      is_stdout_machine_readable: Arc::new(Mutex::new(false)),
      dir_info_error: Arc::new(Mutex::new(None)),
      std_in_pipe: Arc::new(Mutex::new({
        let pipe = create_test_pipe();
        (Some(pipe.0), pipe.1)
      })),
      std_out_pipe: Arc::new(Mutex::new({
        let pipe = create_test_pipe();
        (Some(pipe.0), pipe.1)
      })),
      #[cfg(windows)]
      path_dirs: Default::default(),
      cpu_arch: Arc::new(Mutex::new("x86_64".to_string())),
      os: Arc::new(Mutex::new(std::env::consts::OS.to_string())),
      max_threads_count: Arc::new(Mutex::new(std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4))),
      current_exe_path: Arc::new(Mutex::new(PathBuf::from("/dprint"))),
      is_terminal_interactive: Arc::new(Mutex::new(true)),
      run_command_results: Default::default(),
      running_processes: Default::default(),
      remove_dir_all_failures: Default::default(),
      file_watchers: Default::default(),
      locked_files: Default::default(),
      failing_file_writes: Default::default(),
      failing_file_removes: Default::default(),
      failing_file_locks: Default::default(),
    };
    env.mk_dir_all("/").unwrap();
    env
  }

  pub fn take_stdout_messages(&self) -> Vec<String> {
    self.stdout_messages.lock().drain(..).collect()
  }

  pub fn clear_logs(&self) {
    self.stdout_messages.lock().clear();
    self.stderr_messages.lock().clear();
  }

  pub fn take_stderr_messages(&self) -> Vec<String> {
    self.stderr_messages.lock().drain(..).collect()
  }

  pub fn add_remote_file(&self, path: &str, bytes: &'static [u8]) {
    self.add_remote_file_bytes(path, Vec::from(bytes));
  }

  pub fn add_remote_file_bytes(&self, path: &str, bytes: Vec<u8>) {
    self.remote_files.lock().insert(String::from(path), Ok(bytes));
    // the headers of a previous version don't apply
    self.remote_file_headers.lock().remove(path);
  }

  /// Adds a remote file whose responses have the provided headers. A request
  /// with cache validators matching the `etag` or `last-modified` header gets
  /// a 304 Not Modified response.
  pub fn add_remote_file_with_headers(&self, path: &str, bytes: &[u8], headers: &[(&str, &str)]) {
    self.add_remote_file_bytes(path, bytes.to_vec());
    self.remote_file_headers.lock().insert(
      path.to_string(),
      headers.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect(),
    );
  }

  pub fn remove_remote_file(&self, path: &str) {
    self.remote_files.lock().remove(path);
    self.remote_file_headers.lock().remove(path);
  }

  pub fn add_remote_file_error(&self, path: &str, err: &str) {
    self.remote_files.lock().insert(String::from(path), Err(anyhow!("{}", err)));
  }

  pub fn get_remote_file(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let remote_files = self.remote_files.lock();
    match remote_files.get(&String::from(url)) {
      Some(Ok(result)) => Ok(Some(result.clone())),
      Some(Err(err)) => Err(anyhow!("{:#}", err)),
      None => Ok(None),
    }
  }

  pub fn add_remote_file_redirect(&self, from: &str, to: &str) {
    self.remote_file_redirects.lock().insert(from.to_string(), to.to_string());
  }

  pub fn take_remote_file_auth(&self, url: &str) -> Option<String> {
    self.remote_file_auth.lock().remove(url).flatten()
  }

  /// Takes the debug text of the `DownloadOptions` the url was last requested with.
  pub fn take_remote_file_options(&self, url: &str) -> Option<String> {
    self.remote_file_options.lock().remove(url)
  }

  /// Takes the debug text of the `DownloadProxy` the url was last requested with.
  pub fn take_remote_file_proxy(&self, url: &str) -> Option<String> {
    self.remote_file_proxies.lock().remove(url)
  }

  /// Gets the number of times the URL was requested.
  pub fn remote_file_request_count(&self, url: &str) -> usize {
    self.remote_file_request_counts.lock().get(url).copied().unwrap_or(0)
  }

  /// Gets the number of times the file was read, whether or not it existed.
  pub fn file_read_count(&self, path: impl AsRef<Path>) -> usize {
    self.file_read_counts.lock().get(&self.clean_path(path)).copied().unwrap_or(0)
  }

  /// Makes the path a symlink to another local file or directory that it's
  /// canonicalized to, along with the paths in it (ex. a workspace opened
  /// through a symlink), which canonicalizing doesn't do otherwise.
  pub fn add_canonical_symlink(&self, dir_path: impl AsRef<Path>, canonical_dir_path: impl AsRef<Path>) {
    let (dir_path, canonical_dir_path) = (self.clean_path(dir_path), self.clean_path(canonical_dir_path));
    if let Some(parent) = dir_path.parent() {
      self.mk_dir_all(parent).unwrap();
    }
    self.symlink(&canonical_dir_path, &dir_path).unwrap();
    self.add_canonical_path(dir_path, canonical_dir_path);
  }

  /// Makes the path canonicalize to another one like `add_canonical_symlink`
  /// without making it a symlink, which is for a file because the test
  /// environment doesn't read a file through a symlink.
  pub fn add_canonical_path(&self, path: impl AsRef<Path>, canonical_path: impl AsRef<Path>) {
    self.canonical_paths.lock().push((self.clean_path(path), self.clean_path(canonical_path)));
  }

  /// Removes the canonical path that was added for the path (see
  /// `add_canonical_path`), which is like replacing a symlink with a file.
  pub fn remove_canonical_path(&self, path: impl AsRef<Path>) {
    let path = self.clean_path(path);
    self.canonical_paths.lock().retain(|(added_path, _)| *added_path != path);
  }

  /// Gets the number of times the path was looked up following its symlinks
  /// (ex. `path_exists` or `fs_metadata`), which reaches another machine on a
  /// real file system when it's a symlink to one.
  pub fn stat_count(&self, path: impl AsRef<Path>) -> usize {
    self.stat_counts.lock().get(&self.clean_path(path)).copied().unwrap_or(0)
  }

  /// Gets the number of times the file was opened (ex. to read its shebang
  /// line), whether or not it existed.
  pub fn file_open_count(&self, path: impl AsRef<Path>) -> usize {
    self.file_open_counts.lock().get(&self.clean_path(path)).copied().unwrap_or(0)
  }

  /// Gets the number of times the path was canonicalized, which follows its
  /// symlinks on a real file system (ex. to another machine).
  pub fn canonicalize_count(&self, path: impl AsRef<Path>) -> usize {
    self.canonicalize_counts.lock().get(&self.clean_path(path)).copied().unwrap_or(0)
  }

  /// Gets the number of times the kind of any path was gotten.
  pub fn path_kind_count(&self) -> usize {
    *self.path_kind_count.lock()
  }

  /// Makes the directory the root of an automounter's file system (ex. `/net`
  /// with autofs), whose children may be mounted from another machine (see
  /// `Environment::is_automount_dir`).
  pub fn add_automount_dir(&self, dir_path: impl AsRef<Path>) {
    let dir_path = self.clean_path(dir_path);
    self.automount_dirs.lock().insert(dir_path);
  }

  /// Makes the path where a share is mounted from another machine when it's
  /// looked up (ex. an AFS cell, see `Environment::is_automount_point`).
  pub fn add_automount_point(&self, path: impl AsRef<Path>) {
    let path = self.clean_path(path);
    self.automount_points.lock().insert(path);
  }

  /// Makes requests of the URL not get a response, which models a host
  /// that accepts a connection and then doesn't answer.
  pub fn add_unresponsive_remote_file(&self, url: &str) {
    self.unresponsive_remote_files.lock().push(url.to_string());
  }

  /// Makes the requests of the URL get a response again, including the ones
  /// that are waiting on one.
  pub fn remove_unresponsive_remote_file(&self, url: &str) {
    self.unresponsive_remote_files.lock().retain(|u| u != url);
  }

  pub fn set_env_var(&self, name: &str, value: Option<&str>) {
    match value {
      Some(value) => self.sys.env_set_var(name, value),
      None => self.sys.env_remove_var(name),
    }
  }

  pub fn set_selection_result(&self, index: usize) {
    *self.selection_result.lock() = index;
  }

  /// Gives the prompts that use `prompt_terminal` a terminal of the provided
  /// size that presses the keys of the steps. See
  /// `ScriptedPromptTerminal::new` for what a step is.
  pub fn set_prompt_keys(&self, steps: &[&str], terminal_size: Option<TerminalSize>) -> Result<()> {
    *self.prompt_terminal.lock() = Some(ScriptedPromptTerminal::new(steps, terminal_size)?);
    Ok(())
  }

  /// What was on the screen after each step of `set_prompt_keys`.
  pub fn take_prompt_screens(&self) -> Vec<PromptScreen> {
    self.prompt_terminal.lock().as_ref().map(|terminal| terminal.take_screens()).unwrap_or_default()
  }

  /// Whether a prompt wanted a key after the last step of `set_prompt_keys`.
  pub fn is_out_of_prompt_keys(&self) -> bool {
    self.prompt_terminal.lock().as_ref().is_some_and(|terminal| terminal.is_out_of_steps())
  }

  /// The steps of `set_prompt_keys` that nothing asked for.
  pub fn remaining_prompt_keys(&self) -> Vec<String> {
    self
      .prompt_terminal
      .lock()
      .as_ref()
      .map(|terminal| terminal.remaining_steps())
      .unwrap_or_default()
  }

  pub fn set_confirm_results(&self, values: Vec<Result<Option<bool>>>) {
    *self.confirm_results.lock() = values;
  }

  pub fn set_terminal_interactive(&self, value: bool) {
    *self.is_terminal_interactive.lock() = value;
  }

  pub fn set_cwd(&self, new_path: &str) {
    self.sys.env_set_current_dir(new_path).unwrap();
  }

  /// Pins the in-memory clock so subsequent writes get a deterministic
  /// modification time. Advancing it between writes lets tests exercise
  /// mtime-based cache invalidation without depending on wall-clock. It's the
  /// same clock `sys_time_now` reads, so it also fixes "now" for anything
  /// comparing against it (ex. the minimum dependency age).
  pub fn set_fs_time(&self, secs_since_epoch: u64) {
    self
      .sys
      .set_time(Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs_since_epoch)));
  }

  pub fn set_stdout_machine_readable(&self, value: bool) {
    *self.is_stdout_machine_readable.lock() = value;
  }

  pub fn stdout_reader(&self) -> Box<dyn Read + Send> {
    Box::new(self.std_out_pipe.lock().1.clone())
  }

  pub fn stdin_writer(&self) -> Box<dyn Write + Send> {
    Box::new(self.std_in_pipe.lock().0.take().unwrap())
  }

  #[cfg(windows)]
  pub fn get_system_path_dirs(&self) -> Vec<PathBuf> {
    self.path_dirs.lock().clone()
  }

  pub fn set_staged_file(&self, file: impl AsRef<Path>) {
    self.staged_files.lock().push(file.as_ref().to_path_buf())
  }
  pub fn set_dirty_file(&self, file: impl AsRef<Path>) {
    self.dirty_files.lock().push(file.as_ref().to_path_buf())
  }
  pub fn set_global_gitignore_path(&self, path: impl AsRef<Path>) {
    *self.global_gitignore_path.lock() = Some(path.as_ref().to_path_buf());
  }
  pub fn set_dir_info_error(&self, err: io::Error) {
    *self.dir_info_error.lock() = Some(err);
  }

  pub fn set_current_exe_path(&self, path: impl AsRef<Path>) {
    *self.current_exe_path.lock() = path.as_ref().to_path_buf();
  }

  pub fn set_cpu_arch(&self, value: &str) {
    *self.cpu_arch.lock() = value.to_string();
  }

  pub fn set_os(&self, value: &str) {
    *self.os.lock() = value.to_string();
  }

  pub fn set_max_threads(&self, value: usize) {
    *self.max_threads_count.lock() = value;
  }

  pub fn set_run_command_result(&self, result: io::Result<Option<i32>>) {
    self.run_command_results.lock().push((Vec::new(), result));
  }

  /// Simulates a running process whose executable is at the given path. A
  /// directory containing it can't be removed until the process is killed.
  pub fn add_running_process(&self, exe_path: impl AsRef<Path>) {
    self.add_running_process_with_restarts(exe_path, 0);
  }

  /// Like [`Self::add_running_process`], but the process re-locks its directory
  /// `restarts` times after being killed before it stays dead — simulating an
  /// editor respawning its process plugins between delete attempts.
  pub fn add_running_process_with_restarts(&self, exe_path: impl AsRef<Path>, restarts: usize) {
    self.running_processes.lock().push((self.clean_path(exe_path), restarts));
  }

  pub fn is_process_running(&self, exe_path: impl AsRef<Path>) -> bool {
    let exe_path = self.clean_path(exe_path);
    self.running_processes.lock().iter().any(|(p, _)| *p == exe_path)
  }

  /// Makes the next `count` `remove_dir_all` calls fail with a transient error
  /// before succeeding, regardless of any running process.
  pub fn set_remove_dir_all_failures(&self, count: usize) {
    *self.remove_dir_all_failures.lock() = count;
  }

  /// Sets whether writing the file fails, which includes an atomic write.
  pub fn set_file_write_fails(&self, file_path: impl AsRef<Path>, fails: bool) {
    set_contains(&self.failing_file_writes, self.clean_path(file_path), fails);
  }

  /// Sets whether deleting the file fails.
  pub fn set_file_remove_fails(&self, file_path: impl AsRef<Path>, fails: bool) {
    set_contains(&self.failing_file_removes, self.clean_path(file_path), fails);
  }

  /// Sets whether locking the file fails (see `Environment::try_lock_file`).
  pub fn set_file_lock_fails(&self, file_path: impl AsRef<Path>, fails: bool) {
    set_contains(&self.failing_file_locks, self.clean_path(file_path), fails);
  }

  /// Creates a symlink at `link` that points at `original`, which may be
  /// relative to the symlink's directory.
  pub fn symlink(&self, original: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    self.sys.fs_symlink_file(original, self.clean_path(link))
  }

  pub fn take_run_commands(&self) -> Vec<(Vec<OsString>, io::Result<Option<i32>>)> {
    self.run_command_results.lock().drain(..).collect()
  }

  /// Remember to drop the plugins collection manually if using this with one.
  pub fn run_in_runtime<T>(&self, future: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_time()
      .thread_stack_size(crate::plugins::WASM_PLUGIN_THREAD_STACK_SIZE)
      .build()
      .unwrap();
    rt.block_on(future)
  }

  /// Tells the watchers of the file that it changed, which forgets the ones
  /// that were dropped.
  fn notify_file_watchers(&self, file_path: &Path) {
    self
      .file_watchers
      .lock()
      .retain(|(file_paths, sender)| !file_paths.iter().any(|path| path == file_path) || sender.send(()).is_ok());
  }

  pub fn clean_path(&self, path: impl AsRef<Path>) -> PathBuf {
    // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
    let file_path = PathBuf::from(path.as_ref().to_string_lossy().replace("\\", "/"));
    let path = if !path.as_ref().is_absolute() && !file_path.starts_with("/") {
      self.cwd().join(file_path)
    } else {
      file_path
    }
    .clean();
    PathBuf::from(path.to_string_lossy().replace("\\", "/"))
  }
}

impl TestEnvironment {
  fn ensure_file_write_succeeds(&self, file_path: &Path) -> io::Result<()> {
    match self.failing_file_writes.lock().contains(&self.clean_path(file_path)) {
      true => Err(io::Error::new(io::ErrorKind::StorageFull, format!("Failed writing '{}'.", file_path.display()))),
      false => Ok(()),
    }
  }

  fn ensure_file_remove_succeeds(&self, file_path: &Path) -> io::Result<()> {
    let file_path = self.clean_path(file_path);
    // a file that doesn't exist isn't found instead
    match self.failing_file_removes.lock().contains(&file_path) && self.sys.fs_exists_no_err(&file_path) {
      true => Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("Failed deleting '{}'.", file_path.display()),
      )),
      false => Ok(()),
    }
  }
}

/// Releases a lock of a file when it's dropped (see `Environment::try_lock_file`).
struct TestFileLockGuard {
  file_path: PathBuf,
  locked_files: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Drop for TestFileLockGuard {
  fn drop(&mut self) {
    self.locked_files.lock().remove(&self.file_path);
  }
}

fn set_contains(set: &Mutex<HashSet<PathBuf>>, path: PathBuf, contains: bool) {
  let mut set = set.lock();
  match contains {
    true => set.insert(path),
    false => set.remove(&path),
  };
}

impl Drop for TestEnvironment {
  fn drop(&mut self) {
    // If this panics that means the logged messages or errors weren't inspected for a test.
    // Use take_stdout_messages() or take_stderr_messages() and inspect the results.
    if !std::thread::panicking() && Arc::strong_count(&self.stdout_messages) == 1 {
      assert_eq!(
        self.stdout_messages.lock().clone(),
        Vec::<String>::new(),
        "should not have logged messages left on drop"
      );
      assert_eq!(
        self.stderr_messages.lock().clone(),
        Vec::<String>::new(),
        "should not have logged errors left on drop"
      );
      assert!(self.confirm_results.lock().is_empty(), "should not have confirm results left on drop");
    }
  }
}

impl std::fmt::Debug for TestEnvironment {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("TestEnvironment").finish()
  }
}

impl BaseFsCreateDir for TestEnvironment {
  fn base_fs_create_dir(&self, path: &Path, options: &CreateDirOptions) -> io::Result<()> {
    (*self.sys).base_fs_create_dir(path, options)
  }
}

impl BaseEnvVar for TestEnvironment {
  fn base_env_var_os(&self, key: &OsStr) -> Option<OsString> {
    (*self.sys).base_env_var_os(key)
  }
}

impl BaseFsMetadata for TestEnvironment {
  type Metadata = sys_traits::impls::InMemoryMetadata;

  fn base_fs_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
    *self.stat_counts.lock().entry(self.clean_path(path)).or_default() += 1;
    (*self.sys).base_fs_metadata(path)
  }

  fn base_fs_symlink_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
    (*self.sys).base_fs_symlink_metadata(path)
  }
}

impl BaseFsOpen for TestEnvironment {
  type File = sys_traits::impls::InMemoryFile;

  fn base_fs_open(&self, path: &Path, options: &sys_traits::OpenOptions) -> io::Result<Self::File> {
    *self.file_open_counts.lock().entry(self.clean_path(path)).or_default() += 1;
    (*self.sys).base_fs_open(path, options)
  }
}

impl BaseFsRead for TestEnvironment {
  fn base_fs_read(&self, path: &Path) -> io::Result<Cow<'static, [u8]>> {
    (*self.sys).base_fs_read(path)
  }
}

impl BaseFsRemoveFile for TestEnvironment {
  fn base_fs_remove_file(&self, path: &Path) -> io::Result<()> {
    self.ensure_file_remove_succeeds(path)?;
    (*self.sys).base_fs_remove_file(path)?;
    self.notify_file_watchers(&self.clean_path(path));
    Ok(())
  }
}

impl BaseFsRename for TestEnvironment {
  fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
    self.ensure_file_write_succeeds(to)?;
    (*self.sys).base_fs_rename(from, to)?;
    // ex. an atomic write
    self.notify_file_watchers(&self.clean_path(from));
    self.notify_file_watchers(&self.clean_path(to));
    Ok(())
  }
}

impl BaseFsSetPermissions for TestEnvironment {
  fn base_fs_set_permissions(&self, path: &Path, mode: u32) -> io::Result<()> {
    (*self.sys).base_fs_set_permissions(path, mode)
  }
}

impl ThreadSleep for TestEnvironment {
  fn thread_sleep(&self, duration: std::time::Duration) {
    (*self.sys).thread_sleep(duration);
  }
}

impl SystemRandom for TestEnvironment {
  fn sys_random(&self, buf: &mut [u8]) -> io::Result<()> {
    (*self.sys).sys_random(buf)
  }
}

impl SystemTimeNow for TestEnvironment {
  fn sys_time_now(&self) -> std::time::SystemTime {
    (*self.sys).sys_time_now()
  }
}

#[async_trait(?Send)]
impl UrlDownloader for TestEnvironment {
  async fn download_file_no_redirects(&self, url: &Url, options: DownloadOptions<'_>) -> Result<Option<DownloadedFile>> {
    self.download_policy.lock().ensure_allows_download(url, options.proxy)?;
    self.remote_file_auth.lock().insert(url.to_string(), options.auth.map(|s| s.to_string()));
    self.remote_file_proxies.lock().insert(url.to_string(), format!("{:?}", options.proxy));
    self.remote_file_options.lock().insert(url.to_string(), format!("{:?}", options));
    *self.remote_file_request_counts.lock().entry(url.to_string()).or_default() += 1;

    while self.unresponsive_remote_files.lock().iter().any(|u| u == url.as_str()) {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // check for a redirect first
    let redirects = self.remote_file_redirects.lock();
    if let Some(target) = redirects.get(url.as_str()) {
      return Ok(Some(DownloadedFile {
        headers: [("location".to_string(), target.clone())].into_iter().collect(),
        content: vec![],
        not_modified: false,
      }));
    }
    drop(redirects);

    let Some(content) = self.get_remote_file(url.as_str())? else {
      return Ok(None);
    };
    let headers = self.remote_file_headers.lock().get(url.as_str()).cloned().unwrap_or_default();
    let validators = options.cache_validators;
    let not_modified = (validators.etag.is_some() && validators.etag == headers.get("etag").map(|s| s.as_str()))
      || (validators.last_modified.is_some() && validators.last_modified == headers.get("last-modified").map(|s| s.as_str()));
    Ok(Some(DownloadedFile {
      headers,
      content: if not_modified { vec![] } else { content },
      not_modified,
    }))
  }
}

#[async_trait]
impl Environment for TestEnvironment {
  fn is_real(&self) -> bool {
    false
  }

  fn env_var(&self, name: &str) -> Option<OsString> {
    self.sys.env_var_os(name)
  }

  fn get_staged_files(&self) -> Result<Vec<PathBuf>> {
    Ok(self.staged_files.lock().clone())
  }

  fn get_dirty_files(&self) -> Result<Vec<PathBuf>> {
    Ok(self.dirty_files.lock().clone())
  }

  fn global_gitignore_path(&self) -> Option<PathBuf> {
    self.global_gitignore_path.lock().clone()
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> io::Result<String> {
    let file_bytes = self.read_file_bytes(file_path)?;
    Ok(String::from_utf8(file_bytes.to_vec()).unwrap())
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let file_path = self.clean_path(file_path);
    *self.file_read_counts.lock().entry(file_path.clone()).or_default() += 1;
    self.sys.fs_read(file_path).map(|b| b.into_owned())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let file_path = self.clean_path(file_path);
    self.ensure_file_write_succeeds(&file_path)?;
    self.sys.fs_write(&file_path, bytes)?;
    self.notify_file_watchers(&file_path);
    Ok(())
  }

  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> io::Result<()> {
    let path_from = self.clean_path(path_from);
    let path_to = self.clean_path(path_to);
    self.sys.fs_rename(&path_from, &path_to)?;
    self.notify_file_watchers(&path_from);
    self.notify_file_watchers(&path_to);
    Ok(())
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> io::Result<()> {
    let file_path = self.clean_path(file_path);
    self.ensure_file_remove_succeeds(&file_path)?;
    self.sys.fs_remove_file(&file_path)?;
    self.notify_file_watchers(&file_path);
    Ok(())
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> io::Result<()> {
    let dir_path = self.clean_path(dir_path);
    {
      let mut failures = self.remove_dir_all_failures.lock();
      if *failures > 0 {
        *failures -= 1;
        return Err(io::Error::new(
          io::ErrorKind::PermissionDenied,
          format!("Error deleting directory '{}': transient failure", dir_path.display()),
        ));
      }
    }
    // simulate a running process locking its executable (e.g. on Windows)
    if self.running_processes.lock().iter().any(|(exe, _)| exe.starts_with(&dir_path)) {
      return Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("Error deleting directory '{}': a process is using a file within it", dir_path.display()),
      ));
    }
    self.sys.fs_remove_dir_all(dir_path)
  }

  fn kill_processes_using_dir(&self, dir_path: impl AsRef<Path>) -> usize {
    let dir_path = self.clean_path(dir_path);
    let mut killed = 0;
    self.running_processes.lock().retain_mut(|(exe, restarts)| {
      if !exe.starts_with(&dir_path) {
        return true;
      }
      killed += 1;
      // simulate an editor restarting the plugin: it stays "running" while it
      // still has restarts left, otherwise the kill sticks
      if *restarts > 0 {
        *restarts -= 1;
        true
      } else {
        false
      }
    });
    killed
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> io::Result<Vec<DirEntry>> {
    if let Some(err) = self.dir_info_error.lock().take() {
      return Err(err);
    }

    let dir_path = self.clean_path(dir_path);
    let mut entries = Vec::new();
    for entry in self.sys.fs_read_dir(&dir_path)? {
      let entry = entry?;
      let file_type = entry.file_type()?;
      if file_type.is_dir() {
        entries.push(DirEntry::Directory(self.clean_path(entry.path())));
      } else if file_type.is_file() {
        entries.push(DirEntry::File {
          path: self.clean_path(entry.path()),
        });
      }
    }

    Ok(entries)
  }

  fn try_lock_file(&self, file_path: impl AsRef<Path>) -> io::Result<Option<FileLock>> {
    let file_path = self.clean_path(file_path);
    if self.failing_file_locks.lock().contains(&file_path) {
      return Err(io::Error::new(io::ErrorKind::Unsupported, format!("Failed locking '{}'.", file_path.display())));
    }
    if !self.sys.fs_exists_no_err(&file_path) {
      self.sys.fs_write(&file_path, b"")?;
    }
    if !self.locked_files.lock().insert(file_path.clone()) {
      return Ok(None);
    }
    Ok(Some(FileLock::new(TestFileLockGuard {
      file_path,
      locked_files: self.locked_files.clone(),
    })))
  }

  fn remove_empty_dir(&self, dir_path: impl AsRef<Path>) -> io::Result<()> {
    let dir_path = self.clean_path(dir_path);
    if !self.dir_entry_paths(&dir_path)?.is_empty() {
      return Err(io::Error::new(
        io::ErrorKind::DirectoryNotEmpty,
        format!("Directory not empty: {}", dir_path.display()),
      ));
    }
    self.sys.fs_remove_dir_all(&dir_path)
  }

  fn dir_entry_paths(&self, dir_path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    let dir_path = self.clean_path(dir_path);
    let mut paths = Vec::new();
    for entry in self.sys.fs_read_dir(&dir_path)? {
      paths.push(self.clean_path(entry?.path()));
    }
    Ok(paths)
  }

  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool {
    let path = self.clean_path(file_path);
    *self.stat_counts.lock().entry(path.clone()).or_default() += 1;
    self.sys.fs_exists_no_err(path)
  }

  fn path_is_file(&self, file_path: impl AsRef<Path>) -> bool {
    let path = self.clean_path(file_path);
    *self.stat_counts.lock().entry(path.clone()).or_default() += 1;
    self.sys.fs_is_file_no_err(path)
  }

  fn path_kind(&self, file_path: impl AsRef<Path>) -> Option<PathKind> {
    *self.path_kind_count.lock() += 1;
    let path = self.clean_path(file_path);
    let metadata = self.sys.fs_symlink_metadata(path).ok()?;
    Some(match metadata.file_type() {
      sys_traits::FileType::Dir => PathKind::Dir,
      sys_traits::FileType::Symlink => PathKind::Symlink,
      sys_traits::FileType::File | sys_traits::FileType::Unknown => PathKind::File,
    })
  }

  fn read_link(&self, path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = self.clean_path(path);
    self.sys.fs_read_link(path)
  }

  fn is_automount_dir(&self, dir_path: impl AsRef<Path>) -> bool {
    let dir_path = self.clean_path(dir_path);
    self.automount_dirs.lock().contains(&dir_path)
  }

  fn is_automount_point(&self, path: impl AsRef<Path>) -> bool {
    let path = self.clean_path(path);
    self.automount_points.lock().contains(&path)
  }

  fn canonicalize(&self, path: impl AsRef<Path>) -> io::Result<CanonicalizedPathBuf> {
    let path = self.clean_path(path);
    *self.canonicalize_counts.lock().entry(path.clone()).or_default() += 1;
    let canonical_dir = self
      .canonical_paths
      .lock()
      .iter()
      .find_map(|(dir_path, canonical_dir_path)| Some(canonical_dir_path.join(path.strip_prefix(dir_path).ok()?)));
    if let Some(path) = canonical_dir {
      return Ok(CanonicalizedPathBuf::new(self.clean_path(path)));
    }
    // todo: use sys_traits to implement this properly
    // if !self.path_exists(&path) {
    //   Err(io::Error::new(io::ErrorKind::NotFound, "Path not found."))
    // } else {
    Ok(CanonicalizedPathBuf::new(path))
    // }
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    // cross platform check
    path.as_ref().to_string_lossy().starts_with("/") || path.as_ref().is_absolute()
  }

  fn file_permissions(&self, path: impl AsRef<Path>) -> io::Result<FilePermissions> {
    let path = self.clean_path(path);
    let metadata = self.sys.fs_metadata(path)?;

    let readonly = {
      let mode = metadata.mode()?;
      mode & 0o222 == 0
    };

    Ok(FilePermissions::Test(super::TestFilePermissions { readonly }))
  }

  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> io::Result<()> {
    let path = self.clean_path(path);
    let readonly = match permissions {
      FilePermissions::Std(_) => unreachable!(),
      FilePermissions::Test(permissions) => permissions.readonly,
    };

    let mode = if readonly {
      // Read-only: no write bits set (0o444 = r--r--r--)
      0o444
    } else {
      // Writable: read and write for all (0o666 = rw-rw-rw-)
      0o666
    };

    self.sys.fs_set_permissions(path, mode)
  }

  fn mk_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
    self.sys.fs_create_dir_all(path)
  }

  fn cwd(&self) -> CanonicalizedPathBuf {
    let cwd = self.sys.env_current_dir().unwrap();
    self.canonicalize(cwd).unwrap()
  }

  fn current_exe(&self) -> io::Result<PathBuf> {
    Ok(self.current_exe_path.lock().clone())
  }

  fn __log__(&self, text: &str) {
    if *self.is_stdout_machine_readable.lock() {
      return;
    }
    self.stdout_messages.lock().push(String::from(text));
  }

  fn log_stderr_with_context(&self, level: LogLevel, text: &str, context_name: &str) {
    if let Some(sink) = &*self.output_sink.lock() {
      sink.log(level, &get_sink_text(text, context_name));
      return;
    }
    self.stderr_messages.lock().push(get_stderr_text(level, text).into_owned());
  }

  fn set_output_sink(&self, sink: Arc<dyn OutputSink>) {
    *self.output_sink.lock() = Some(sink.clone());
    self.progress_bars.set(Some(Arc::new(ProgressBars::with_sink(sink))));
  }

  fn download_policy(&self) -> DownloadPolicy {
    self.download_policy.lock().clone()
  }

  fn set_download_policy(&self, policy: DownloadPolicy) {
    *self.download_policy.lock() = policy;
  }

  fn watch_files(&self, file_paths: &[PathBuf]) -> io::Result<FilesWatcher> {
    let (sender, changes) = tokio::sync::mpsc::unbounded_channel();
    let file_paths = file_paths.iter().map(|path| self.clean_path(path)).collect();
    self.file_watchers.lock().push((file_paths, sender));
    Ok(FilesWatcher::new(changes, ()))
  }

  fn log_machine_readable(&self, text: &[u8]) {
    assert!(*self.is_stdout_machine_readable.lock());
    self.stdout_messages.lock().push(String::from_utf8(text.to_vec()).unwrap());
  }

  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    total_size: usize,
  ) -> TResult {
    if let Some(progress_bars) = self.progress_bars() {
      return log_action_with_progress(Some(&progress_bars), message, action, total_size);
    }
    self.__log_stderr__(message);
    action(Box::new(|_| {}))
  }

  fn get_cache_dir(&self) -> CanonicalizedPathBuf {
    self.canonicalize("/cache").unwrap()
  }

  fn get_config_dir(&self) -> Option<PathBuf> {
    Some(PathBuf::from("/config"))
  }

  fn get_home_dir(&self) -> Option<CanonicalizedPathBuf> {
    self.canonicalize("/home").ok()
  }

  fn cpu_arch(&self) -> String {
    self.cpu_arch.lock().clone()
  }

  fn os(&self) -> String {
    self.os.lock().clone()
  }

  fn available_parallelism(&self) -> Option<NonZeroUsize> {
    NonZeroUsize::new(*self.max_threads_count.lock())
  }

  fn cli_version(&self) -> String {
    "0.0.0".to_string()
  }

  fn get_time_secs(&self) -> u64 {
    123456
  }

  fn get_selection(&self, prompt_message: &str, _: u16, _: &[String]) -> Result<usize> {
    self.__log_stderr__(prompt_message);
    Ok(*self.selection_result.lock())
  }

  fn prompt_terminal(&self) -> Box<dyn PromptTerminal + '_> {
    match self.prompt_terminal.lock().clone() {
      Some(prompt_terminal) => Box::new(prompt_terminal),
      None => panic!("A prompt was shown, but the test didn't say what to press in it. Use set_prompt_keys."),
    }
  }

  fn confirm_with_strategy(&self, strategy: &dyn ShowConfirmStrategy) -> Result<bool> {
    let mut confirm_results = self.confirm_results.lock();
    let result = confirm_results.remove(0).map(|v| v.unwrap_or(strategy.default_value()));
    self.__log_stderr__(&strategy.render(match &result {
      Ok(value) => Some(*value),
      Err(_) => None,
    }));
    result
  }

  fn run_command_get_status(&self, args: Vec<OsString>) -> io::Result<Option<i32>> {
    let mut results = self.run_command_results.lock();
    if results.is_empty() {
      panic!(
        "run_command_get_status called with args {:?} but no result was set. Use set_run_command_result to set expected results.",
        args
      );
    }
    let (expected_args, result) = results.remove(0);
    // Verify the actual args match expected args if they were provided
    if !expected_args.is_empty() && expected_args != args {
      panic!("Expected command args {:?} but got {:?}", expected_args, args);
    }
    // Store the actual command that was run for later verification
    results.push((args, Ok(Some(0))));
    result
  }

  fn is_ci(&self) -> bool {
    false
  }

  fn is_terminal_interactive(&self) -> bool {
    *self.is_terminal_interactive.lock()
  }

  fn log_level(&self) -> LogLevel {
    *self.log_level.lock()
  }

  fn set_log_level(&self, log_level: LogLevel) {
    *self.log_level.lock() = log_level;
  }

  fn progress_bars(&self) -> Option<Arc<ProgressBars>> {
    self.progress_bars.get()
  }

  fn compile_wasm(&self, bytes: &[u8]) -> Result<CompilationResult> {
    use std::collections::hash_map::Entry;

    static COMPILE_RESULTS: Lazy<Mutex<HashMap<u64, CompilationResult>>> = Lazy::new(Default::default);

    let hash = get_bytes_hash(bytes);
    {
      // hold the lock while compiling in order to prevent other
      // threads from compiling at the same time
      let mut results = COMPILE_RESULTS.lock();
      let entry = results.entry(hash);
      match entry {
        Entry::Occupied(entry) => Ok(entry.get().clone()),
        Entry::Vacant(entry) => {
          let value = crate::plugins::compile_wasm(bytes, self.max_threads()).unwrap();
          entry.insert(value.clone());
          Ok(value)
        }
      }
    }
  }

  fn wasm_cache_key(&self) -> String {
    self.cpu_arch()
  }

  async fn cpu_usage(&self) -> u8 {
    20
  }

  fn stdout(&self) -> Box<dyn Write + Send> {
    Box::new(self.std_out_pipe.lock().0.take().unwrap())
  }

  fn stdin(&self) -> Box<dyn Read + Send> {
    Box::new(self.std_in_pipe.lock().1.clone())
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> io::Result<()> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if !path_dirs.contains(&directory_path) {
      path_dirs.push(directory_path);
    }
    Ok(())
  }

  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> io::Result<()> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if let Some(pos) = path_dirs.iter().position(|p| p == &directory_path) {
      path_dirs.remove(pos);
    }
    Ok(())
  }
}
