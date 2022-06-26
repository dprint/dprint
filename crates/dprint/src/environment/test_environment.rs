use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use futures::Future;
use parking_lot::Condvar;
use parking_lot::Mutex;
use path_clean::PathClean;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::CanonicalizedPathBuf;
use super::DirEntry;
use super::DirEntryKind;
use super::Environment;
use super::FilePermissions;
use super::UrlDownloader;
use crate::plugins::CompilationResult;

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
  fn read(&mut self, _: &mut [u8]) -> Result<usize, std::io::Error> {
    panic!("Not implemented");
  }

  fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), std::io::Error> {
    let mut buffer_data = self.0.buffer_data.lock();

    while buffer_data.data.len() < buffer_data.read_pos + buf.len() && !buffer_data.closed {
      self.0.cond_var.wait(&mut buffer_data);
    }

    if buffer_data.data.len() == buffer_data.read_pos && buffer_data.closed {
      return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Broken pipe."));
    }

    buf.copy_from_slice(&buffer_data.data[buffer_data.read_pos..buffer_data.read_pos + buf.len()]);
    buffer_data.read_pos += buf.len();

    Ok(())
  }
}

impl Write for TestPipeWriter {
  fn write(&mut self, data: &[u8]) -> Result<usize, std::io::Error> {
    let result = {
      let mut buffer_data = self.0.buffer_data.lock();
      buffer_data.data.write(data)
    };
    self.0.cond_var.notify_one();
    result
  }

  fn flush(&mut self) -> Result<(), std::io::Error> {
    Ok(())
  }
}

#[derive(Clone)]
pub struct TestEnvironment {
  is_verbose: Arc<Mutex<bool>>,
  cwd: Arc<Mutex<String>>,
  files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
  file_permissions: Arc<Mutex<HashMap<PathBuf, FilePermissions>>>,
  stdout_messages: Arc<Mutex<Vec<String>>>,
  stderr_messages: Arc<Mutex<Vec<String>>>,
  remote_files: Arc<Mutex<HashMap<String, Result<Vec<u8>>>>>,
  deleted_directories: Arc<Mutex<Vec<PathBuf>>>,
  selection_result: Arc<Mutex<usize>>,
  multi_selection_result: Arc<Mutex<Option<Vec<usize>>>>,
  confirm_results: Arc<Mutex<Vec<Result<Option<bool>>>>>,
  is_stdout_machine_readable: Arc<Mutex<bool>>,
  wasm_compile_result: Arc<Mutex<Option<CompilationResult>>>,
  dir_info_error: Arc<Mutex<Option<Error>>>,
  std_in_pipe: Arc<Mutex<(Option<TestPipeWriter>, TestPipeReader)>>,
  std_out_pipe: Arc<Mutex<(Option<TestPipeWriter>, TestPipeReader)>>,
  runtime_handle: Arc<Mutex<Option<tokio::runtime::Handle>>>,
  #[cfg(windows)]
  path_dirs: Arc<Mutex<Vec<PathBuf>>>,
  cpu_arch: Arc<Mutex<String>>,
  core_count: Arc<Mutex<usize>>,
  current_exe_path: Arc<Mutex<PathBuf>>,
}

impl TestEnvironment {
  pub fn new() -> TestEnvironment {
    TestEnvironment {
      is_verbose: Arc::new(Mutex::new(false)),
      cwd: Arc::new(Mutex::new(String::from("/"))),
      files: Default::default(),
      file_permissions: Default::default(),
      stdout_messages: Default::default(),
      stderr_messages: Default::default(),
      remote_files: Default::default(),
      deleted_directories: Default::default(),
      selection_result: Arc::new(Mutex::new(0)),
      multi_selection_result: Arc::new(Mutex::new(None)),
      confirm_results: Default::default(),
      is_stdout_machine_readable: Arc::new(Mutex::new(false)),
      wasm_compile_result: Arc::new(Mutex::new(None)),
      dir_info_error: Arc::new(Mutex::new(None)),
      std_in_pipe: Arc::new(Mutex::new({
        let pipe = create_test_pipe();
        (Some(pipe.0), pipe.1)
      })),
      std_out_pipe: Arc::new(Mutex::new({
        let pipe = create_test_pipe();
        (Some(pipe.0), pipe.1)
      })),
      runtime_handle: Default::default(),
      #[cfg(windows)]
      path_dirs: Default::default(),
      cpu_arch: Arc::new(Mutex::new("x86_64".to_string())),
      core_count: Arc::new(Mutex::new(std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4))),
      current_exe_path: Arc::new(Mutex::new(PathBuf::from("/dprint"))),
    }
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
    let mut remote_files = self.remote_files.lock();
    remote_files.insert(String::from(path), Ok(bytes));
  }

  pub fn add_remote_file_error(&self, path: &str, err: &str) {
    let mut remote_files = self.remote_files.lock();
    remote_files.insert(String::from(path), Err(anyhow!("{}", err)));
  }

  pub fn is_dir_deleted(&self, path: impl AsRef<Path>) -> bool {
    let deleted_directories = self.deleted_directories.lock();
    deleted_directories.contains(&path.as_ref().to_path_buf())
  }

  pub fn set_selection_result(&self, index: usize) {
    let mut selection_result = self.selection_result.lock();
    *selection_result = index;
  }

  pub fn set_multi_selection_result(&self, indexes: Vec<usize>) {
    let mut multi_selection_result = self.multi_selection_result.lock();
    *multi_selection_result = Some(indexes);
  }

  pub fn set_confirm_results(&self, values: Vec<Result<Option<bool>>>) {
    let mut confirm_results = self.confirm_results.lock();
    *confirm_results = values;
  }

  pub fn set_cwd(&self, new_path: &str) {
    let mut cwd = self.cwd.lock();
    *cwd = String::from(new_path);
  }

  pub fn set_stdout_machine_readable(&self, value: bool) {
    *self.is_stdout_machine_readable.lock() = value;
  }

  pub fn set_verbose(&self, value: bool) {
    let mut is_verbose = self.is_verbose.lock();
    *is_verbose = value;
  }

  pub fn set_wasm_compile_result(&self, value: CompilationResult) {
    let mut wasm_compile_result = self.wasm_compile_result.lock();
    *wasm_compile_result = Some(value);
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

  pub fn set_dir_info_error(&self, err: Error) {
    let mut dir_info_error = self.dir_info_error.lock();
    *dir_info_error = Some(err);
  }

  pub fn set_current_exe_path(&self, path: impl AsRef<Path>) {
    *self.current_exe_path.lock() = path.as_ref().to_path_buf();
  }

  pub fn set_cpu_arch(&self, value: &str) {
    *self.cpu_arch.lock() = value.to_string();
  }

  pub fn set_available_parallelism(&self, value: usize) {
    *self.core_count.lock() = value;
  }

  pub fn set_runtime_handle(&self, handle: tokio::runtime::Handle) {
    *self.runtime_handle.lock() = Some(handle);
  }

  /// Remember to drop the plugins collection manually if using this with one.
  pub fn run_in_runtime<T>(&self, future: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_time().build().unwrap();
    self.set_runtime_handle(rt.handle().clone());
    rt.block_on(future)
  }

  fn clean_path(&self, path: impl AsRef<Path>) -> PathBuf {
    // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
    let file_path = PathBuf::from(path.as_ref().to_string_lossy().replace("\\", "/"));
    if !path.as_ref().is_absolute() && !file_path.starts_with("/") {
      self.cwd().join(file_path)
    } else {
      file_path
    }
    .clean()
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

impl UrlDownloader for TestEnvironment {
  fn download_file(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let remote_files = self.remote_files.lock();
    match remote_files.get(&String::from(url)) {
      Some(Ok(result)) => Ok(Some(result.clone())),
      Some(Err(err)) => Err(anyhow!("{:#}", err)),
      None => Ok(None),
    }
  }
}

impl Environment for TestEnvironment {
  fn is_real(&self) -> bool {
    false
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String> {
    let file_bytes = self.read_file_bytes(file_path)?;
    Ok(String::from_utf8(file_bytes.to_vec()).unwrap())
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let file_path = self.clean_path(file_path);
    let files = self.files.lock();
    match files.get(&file_path) {
      Some(text) => Ok(text.clone()),
      None => bail!("Could not find file at path {}", file_path.display()),
    }
  }

  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<()> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let file_path = self.clean_path(file_path);
    let mut files = self.files.lock();
    files.insert(file_path, Vec::from(bytes));
    Ok(())
  }

  fn rename(&self, path_from: impl AsRef<Path>, path_to: impl AsRef<Path>) -> Result<()> {
    let path_from = self.clean_path(path_from);
    let path_to = self.clean_path(path_to);
    {
      let mut files = self.files.lock();
      if let Some(file) = files.remove(&path_from) {
        files.insert(path_to.clone(), file);
      }
    }
    {
      let mut file_permissions = self.file_permissions.lock();
      if let Some(perms) = file_permissions.remove(&path_from) {
        file_permissions.insert(path_to.clone(), perms);
      }
    }
    Ok(())
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<()> {
    let file_path = self.clean_path(file_path);
    self.files.lock().remove(&file_path);
    self.file_permissions.lock().remove(&file_path);
    Ok(())
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<()> {
    let dir_path = self.clean_path(dir_path);
    {
      let mut deleted_directories = self.deleted_directories.lock();
      deleted_directories.push(dir_path.clone());
    }
    let mut files = self.files.lock();
    let mut delete_paths = Vec::new();
    for (file_path, _) in files.iter() {
      if file_path.starts_with(&dir_path) {
        delete_paths.push(file_path.clone());
      }
    }
    for path in delete_paths {
      files.remove(&path);
    }
    Ok(())
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>> {
    if let Some(err) = self.dir_info_error.lock().take() {
      return Err(err);
    }

    let mut entries = Vec::new();
    let mut found_directories = HashSet::new();
    let dir_path = self.clean_path(dir_path);

    let files = self.files.lock();
    for key in files.keys() {
      if key.parent().unwrap() == dir_path {
        entries.push(DirEntry {
          kind: DirEntryKind::File,
          path: key.clone(),
        });
      } else {
        let mut current_dir = key.parent();
        while let Some(ancestor_dir) = current_dir {
          let ancestor_parent_dir = match ancestor_dir.parent() {
            Some(dir) => dir.to_path_buf(),
            None => break,
          };

          if ancestor_parent_dir == dir_path && found_directories.insert(ancestor_dir) {
            entries.push(DirEntry {
              kind: DirEntryKind::Directory,
              path: ancestor_dir.to_path_buf(),
            });
            break;
          }
          current_dir = ancestor_dir.parent();
        }
      }
    }

    Ok(entries)
  }

  fn path_exists(&self, file_path: impl AsRef<Path>) -> bool {
    let files = self.files.lock();
    files.contains_key(&self.clean_path(file_path))
  }

  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<CanonicalizedPathBuf> {
    Ok(CanonicalizedPathBuf::new(self.clean_path(path)))
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    // cross platform check
    path.as_ref().to_string_lossy().starts_with("/") || path.as_ref().is_absolute()
  }

  fn file_permissions(&self, path: impl AsRef<Path>) -> Result<FilePermissions> {
    let path = self.clean_path(path);
    if let Some(permissions) = self.file_permissions.lock().get(&path).cloned() {
      Ok(permissions)
    } else if self.files.lock().contains_key(&path) {
      Ok(FilePermissions::Test(Default::default()))
    } else {
      bail!("File not found.")
    }
  }

  fn set_file_permissions(&self, path: impl AsRef<Path>, permissions: FilePermissions) -> Result<()> {
    let path = self.clean_path(path);
    self.file_permissions.lock().insert(path, permissions);
    Ok(())
  }

  fn mk_dir_all(&self, _: impl AsRef<Path>) -> Result<()> {
    Ok(())
  }

  fn cwd(&self) -> CanonicalizedPathBuf {
    let cwd = self.cwd.lock();
    self.canonicalize(cwd.to_owned()).unwrap()
  }

  fn current_exe(&self) -> Result<PathBuf> {
    Ok(self.current_exe_path.lock().clone())
  }

  fn log(&self, text: &str) {
    if *self.is_stdout_machine_readable.lock() {
      return;
    }
    self.stdout_messages.lock().push(String::from(text));
  }

  fn log_stderr_with_context(&self, text: &str, _: &str) {
    self.stderr_messages.lock().push(String::from(text));
  }

  fn log_machine_readable(&self, text: &str) {
    assert!(*self.is_stdout_machine_readable.lock());
    self.stdout_messages.lock().push(String::from(text));
  }

  fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
    &self,
    message: &str,
    action: TCreate,
    _: usize,
  ) -> TResult {
    self.log_stderr(message);
    action(Box::new(|_| {}))
  }

  fn get_cache_dir(&self) -> PathBuf {
    PathBuf::from("/cache")
  }

  fn cpu_arch(&self) -> String {
    self.cpu_arch.lock().clone()
  }

  fn os(&self) -> String {
    std::env::consts::OS.to_string()
  }

  fn available_parallelism(&self) -> usize {
    *self.core_count.lock()
  }

  fn cli_version(&self) -> String {
    "0.0.0".to_string()
  }

  fn get_time_secs(&self) -> u64 {
    123456
  }

  fn get_terminal_width(&self) -> u16 {
    60
  }

  fn get_selection(&self, prompt_message: &str, _: u16, _: &[String]) -> Result<usize> {
    self.log_stderr(prompt_message);
    Ok(*self.selection_result.lock())
  }

  fn get_multi_selection(&self, prompt_message: &str, _: u16, items: &[(bool, String)]) -> Result<Vec<usize>> {
    self.log_stderr(prompt_message);
    let default_values = items
      .iter()
      .enumerate()
      .filter_map(|(i, (selected, _))| if *selected { Some(i) } else { None })
      .collect();
    Ok(self.multi_selection_result.lock().clone().unwrap_or(default_values))
  }

  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool> {
    let mut confirm_results = self.confirm_results.lock();
    let result = confirm_results.remove(0).map(|v| v.unwrap_or(default_value));
    self.log_stderr(&format!(
      "{} {}",
      prompt_message,
      match &result {
        Ok(true) => "Y".to_string(),
        Ok(false) => "N".to_string(),
        Err(err) => err.to_string(),
      }
    ));
    result
  }

  fn is_verbose(&self) -> bool {
    *self.is_verbose.lock()
  }

  fn compile_wasm(&self, _: &[u8]) -> Result<CompilationResult> {
    let wasm_compile_result = self.wasm_compile_result.lock();
    Ok(wasm_compile_result.clone().expect("Expected compilation result to be set."))
  }

  fn stdout(&self) -> Box<dyn Write + Send> {
    Box::new(self.std_out_pipe.lock().0.take().unwrap())
  }

  fn stdin(&self) -> Box<dyn Read + Send> {
    Box::new(self.std_in_pipe.lock().1.clone())
  }

  fn runtime_handle(&self) -> tokio::runtime::Handle {
    // need to call set_runtime_handle to make this not panic
    self.runtime_handle.lock().as_ref().unwrap().clone()
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<()> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if !path_dirs.contains(&directory_path) {
      path_dirs.push(directory_path);
    }
    Ok(())
  }

  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<()> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if let Some(pos) = path_dirs.iter().position(|p| p == &directory_path) {
      path_dirs.remove(pos);
    }
    Ok(())
  }
}
