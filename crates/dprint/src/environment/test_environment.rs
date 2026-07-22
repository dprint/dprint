use anyhow::Result;
use anyhow::anyhow;
use once_cell::sync::Lazy;
use parking_lot::Condvar;
use parking_lot::Mutex;
use path_clean::PathClean;
use std::borrow::Cow;
use std::collections::HashMap;
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
use sys_traits::FsRemoveDirAll;
use sys_traits::FsRemoveFile;
use sys_traits::FsRename;
use sys_traits::FsSetPermissions;
use sys_traits::FsWrite;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;
use sys_traits::impls::InMemorySys;
use url::Url;

use dprint_core::async_runtime::async_trait;

use super::CanonicalizedPathBuf;
use super::DirEntry;
use super::DownloadedFile;
use super::Environment;
use super::FilePermissions;
use super::PathKind;
use super::UrlDownloader;
use crate::plugins::CompilationResult;
use crate::utils::LogLevel;
use crate::utils::ShowConfirmStrategy;
use crate::utils::get_bytes_hash;

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
  sys: Arc<InMemorySys>,
  staged_files: Arc<Mutex<Vec<PathBuf>>>,
  dirty_files: Arc<Mutex<Vec<PathBuf>>>,
  global_gitignore_path: Arc<Mutex<Option<PathBuf>>>,
  stdout_messages: Arc<Mutex<Vec<String>>>,
  stderr_messages: Arc<Mutex<Vec<String>>>,
  remote_files: Arc<Mutex<HashMap<String, Result<Vec<u8>>>>>,
  remote_file_redirects: Arc<Mutex<HashMap<String, String>>>,
  /// Last auth header seen for each URL.
  remote_file_auth: Arc<Mutex<HashMap<String, Option<String>>>>,
  selection_result: Arc<Mutex<usize>>,
  multi_selection_result: Arc<Mutex<Option<Vec<usize>>>>,
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
}

impl TestEnvironment {
  pub fn new() -> TestEnvironment {
    let env = TestEnvironment {
      log_level: Arc::new(Mutex::new(LogLevel::Info)),
      sys: Default::default(),
      staged_files: Default::default(),
      dirty_files: Default::default(),
      global_gitignore_path: Default::default(),
      stdout_messages: Default::default(),
      stderr_messages: Default::default(),
      remote_files: Default::default(),
      remote_file_redirects: Default::default(),
      remote_file_auth: Default::default(),
      selection_result: Arc::new(Mutex::new(0)),
      multi_selection_result: Arc::new(Mutex::new(None)),
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

  pub fn set_env_var(&self, name: &str, value: Option<&str>) {
    match value {
      Some(value) => self.sys.env_set_var(name, value),
      None => self.sys.env_remove_var(name),
    }
  }

  pub fn set_selection_result(&self, index: usize) {
    *self.selection_result.lock() = index;
  }

  pub fn set_multi_selection_result(&self, indexes: Vec<usize>) {
    *self.multi_selection_result.lock() = Some(indexes);
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

  /// Pins the in-memory filesystem clock so subsequent writes get a
  /// deterministic modification time. Advancing it between writes lets tests
  /// exercise mtime-based cache invalidation without depending on wall-clock.
  pub fn set_fs_time(&self, secs_since_epoch: u64) {
    self
      .sys
      .set_time(Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs_since_epoch)));
  }

  pub fn set_stdout_machine_readable(&self, value: bool) {
    *self.is_stdout_machine_readable.lock() = value;
  }

  pub fn set_log_level(&self, value: LogLevel) {
    *self.log_level.lock() = value;
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
    (*self.sys).base_fs_metadata(path)
  }

  fn base_fs_symlink_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
    (*self.sys).base_fs_symlink_metadata(path)
  }
}

impl BaseFsOpen for TestEnvironment {
  type File = sys_traits::impls::InMemoryFile;

  fn base_fs_open(&self, path: &Path, options: &sys_traits::OpenOptions) -> io::Result<Self::File> {
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
    (*self.sys).base_fs_remove_file(path)
  }
}

impl BaseFsRename for TestEnvironment {
  fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
    (*self.sys).base_fs_rename(from, to)
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
  async fn download_file_no_redirects(&self, url: &Url, auth: Option<&str>) -> Result<Option<DownloadedFile>> {
    self.remote_file_auth.lock().insert(url.to_string(), auth.map(|s| s.to_string()));

    // check for a redirect first
    let redirects = self.remote_file_redirects.lock();
    if let Some(target) = redirects.get(url.as_str()) {
      return Ok(Some(DownloadedFile {
        headers: [("location".to_string(), target.clone())].into_iter().collect(),
        content: vec![],
      }));
    }
    drop(redirects);

    Ok(self.get_remote_file(url.as_str())?.map(|content| DownloadedFile {
      headers: Default::default(),
      content,
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
    self.sys.fs_read(file_path).map(|b| b.into_owned())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let file_path = self.clean_path(file_path);
    self.sys.fs_write(file_path, bytes)
  }

  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> io::Result<()> {
    let path_from = self.clean_path(path_from);
    let path_to = self.clean_path(path_to);
    self.sys.fs_rename(&path_from, &path_to)
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> io::Result<()> {
    let file_path = self.clean_path(file_path);
    self.sys.fs_remove_file(file_path)
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
          name: entry.file_name().into_owned(),
          path: self.clean_path(entry.path()),
        });
      }
    }

    Ok(entries)
  }

  fn path_is_file(&self, file_path: impl AsRef<Path>) -> bool {
    let path = self.clean_path(file_path);
    self.sys.fs_is_file_no_err(path)
  }

  fn path_kind(&self, file_path: impl AsRef<Path>) -> Option<PathKind> {
    let path = self.clean_path(file_path);
    let metadata = self.sys.fs_symlink_metadata(path).ok()?;
    Some(match metadata.file_type() {
      sys_traits::FileType::Dir => PathKind::Dir,
      sys_traits::FileType::Symlink => PathKind::Symlink,
      sys_traits::FileType::File | sys_traits::FileType::Unknown => PathKind::File,
    })
  }

  fn canonicalize(&self, path: impl AsRef<Path>) -> io::Result<CanonicalizedPathBuf> {
    let path = self.clean_path(path);
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

  fn log_stderr_with_context(&self, text: &str, _: &str) {
    self.stderr_messages.lock().push(String::from(text));
  }

  fn log_machine_readable(&self, text: &[u8]) {
    assert!(*self.is_stdout_machine_readable.lock());
    self.stdout_messages.lock().push(String::from_utf8(text.to_vec()).unwrap());
  }

  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    _: usize,
  ) -> TResult {
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

  fn get_multi_selection(&self, prompt_message: &str, _: u16, items: &[(bool, String)]) -> Result<Vec<usize>> {
    self.__log_stderr__(prompt_message);
    let default_values = items
      .iter()
      .enumerate()
      .filter_map(|(i, (selected, _))| if *selected { Some(i) } else { None })
      .collect();
    Ok(self.multi_selection_result.lock().clone().unwrap_or(default_values))
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
          let value = crate::plugins::compile_wasm(bytes).unwrap();
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
