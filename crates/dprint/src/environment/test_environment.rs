use dprint_core::types::ErrBox;
use parking_lot::Mutex;
use path_clean::PathClean;
use std::collections::{HashMap, HashSet};
use std::io::{Error, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::{DirEntry, DirEntryKind, Environment};
use crate::plugins::CompilationResult;

struct BufferData {
  data: Vec<u8>,
  read_pos: usize,
}

#[derive(Clone)]
struct MockStdInOut {
  buffer_data: Arc<Mutex<BufferData>>,
  sender: Arc<Mutex<Sender<u32>>>,
  receiver: Arc<Mutex<Receiver<u32>>>,
}

impl MockStdInOut {
  pub fn new() -> Self {
    let (sender, receiver) = channel();
    MockStdInOut {
      buffer_data: Arc::new(Mutex::new(BufferData { data: Vec::new(), read_pos: 0 })),
      sender: Arc::new(Mutex::new(sender)),
      receiver: Arc::new(Mutex::new(receiver)),
    }
  }
}

impl Read for MockStdInOut {
  fn read(&mut self, _: &mut [u8]) -> Result<usize, Error> {
    panic!("Not implemented");
  }

  fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
    let rx = self.receiver.lock();
    rx.recv().unwrap();

    let mut buffer_data = self.buffer_data.lock();
    buf.copy_from_slice(&buffer_data.data[buffer_data.read_pos..buffer_data.read_pos + buf.len()]);
    buffer_data.read_pos += buf.len();

    Ok(())
  }
}

impl Write for MockStdInOut {
  fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
    let result = {
      let mut buffer_data = self.buffer_data.lock();
      buffer_data.data.write(data)
    };
    let tx = self.sender.lock();
    tx.send(0).unwrap();
    result
  }

  fn flush(&mut self) -> Result<(), Error> {
    Ok(())
  }
}

#[derive(Clone)]
pub struct TestEnvironment {
  is_verbose: Arc<Mutex<bool>>,
  cwd: Arc<Mutex<String>>,
  files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
  logged_messages: Arc<Mutex<Vec<String>>>,
  logged_errors: Arc<Mutex<Vec<String>>>,
  remote_files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
  deleted_directories: Arc<Mutex<Vec<PathBuf>>>,
  selection_result: Arc<Mutex<usize>>,
  multi_selection_result: Arc<Mutex<Vec<usize>>>,
  confirm_results: Arc<Mutex<Vec<Result<Option<bool>, ErrBox>>>>,
  is_silent: Arc<Mutex<bool>>,
  wasm_compile_result: Arc<Mutex<Option<CompilationResult>>>,
  std_in: MockStdInOut,
  std_out: MockStdInOut,
  #[cfg(windows)]
  path_dirs: Arc<Mutex<Vec<PathBuf>>>,
}

impl TestEnvironment {
  pub fn new() -> TestEnvironment {
    TestEnvironment {
      is_verbose: Arc::new(Mutex::new(false)),
      cwd: Arc::new(Mutex::new(String::from("/"))),
      files: Arc::new(Mutex::new(HashMap::new())),
      logged_messages: Arc::new(Mutex::new(Vec::new())),
      logged_errors: Arc::new(Mutex::new(Vec::new())),
      remote_files: Arc::new(Mutex::new(HashMap::new())),
      deleted_directories: Arc::new(Mutex::new(Vec::new())),
      selection_result: Arc::new(Mutex::new(0)),
      multi_selection_result: Arc::new(Mutex::new(Vec::new())),
      confirm_results: Arc::new(Mutex::new(Vec::new())),
      is_silent: Arc::new(Mutex::new(false)),
      wasm_compile_result: Arc::new(Mutex::new(None)),
      std_in: MockStdInOut::new(),
      std_out: MockStdInOut::new(),
      #[cfg(windows)]
      path_dirs: Arc::new(Mutex::new(Vec::new())),
    }
  }

  pub fn take_logged_messages(&self) -> Vec<String> {
    self.logged_messages.lock().drain(..).collect()
  }

  pub fn clear_logs(&self) {
    self.logged_messages.lock().clear();
    self.logged_errors.lock().clear();
  }

  pub fn take_logged_errors(&self) -> Vec<String> {
    self.logged_errors.lock().drain(..).collect()
  }

  pub fn add_remote_file(&self, path: &str, bytes: &'static [u8]) {
    self.add_remote_file_bytes(path, Vec::from(bytes));
  }

  pub fn add_remote_file_bytes(&self, path: &str, bytes: Vec<u8>) {
    let mut remote_files = self.remote_files.lock();
    remote_files.insert(String::from(path), bytes);
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
    *multi_selection_result = indexes;
  }

  pub fn set_confirm_results(&self, values: Vec<Result<Option<bool>, ErrBox>>) {
    let mut confirm_results = self.confirm_results.lock();
    *confirm_results = values;
  }

  pub fn set_cwd(&self, new_path: &str) {
    let mut cwd = self.cwd.lock();
    *cwd = String::from(new_path);
  }

  pub fn set_silent(&self, value: bool) {
    let mut is_silent = self.is_silent.lock();
    *is_silent = value;
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
    Box::new(self.std_out.clone())
  }

  pub fn stdin_writer(&self) -> Box<dyn Write + Send> {
    Box::new(self.std_in.clone())
  }

  #[cfg(windows)]
  pub fn get_system_path_dirs(&self) -> Vec<PathBuf> {
    self.path_dirs.lock().clone()
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
    // Use take_logged_messages() or take_logged_errors() and inspect the results.
    if !std::thread::panicking() && Arc::strong_count(&self.logged_messages) == 1 {
      assert_eq!(
        self.logged_messages.lock().clone(),
        Vec::<String>::new(),
        "should not have logged messages left on drop"
      );
      assert_eq!(
        self.logged_errors.lock().clone(),
        Vec::<String>::new(),
        "should not have logged errors left on drop"
      );
      assert!(self.confirm_results.lock().is_empty(), "should not have confirm results left on drop");
    }
  }
}

impl Environment for TestEnvironment {
  fn is_real(&self) -> bool {
    false
  }

  fn read_file(&self, file_path: impl AsRef<Path>) -> Result<String, ErrBox> {
    let file_bytes = self.read_file_bytes(file_path)?;
    Ok(String::from_utf8(file_bytes.to_vec()).unwrap())
  }

  fn read_file_bytes(&self, file_path: impl AsRef<Path>) -> Result<Vec<u8>, ErrBox> {
    let file_path = self.clean_path(file_path);
    let files = self.files.lock();
    match files.get(&file_path) {
      Some(text) => Ok(text.clone()),
      None => err!("Could not find file at path {}", file_path.display()),
    }
  }

  fn write_file(&self, file_path: impl AsRef<Path>, file_text: &str) -> Result<(), ErrBox> {
    self.write_file_bytes(file_path, file_text.as_bytes())
  }

  fn write_file_bytes(&self, file_path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), ErrBox> {
    let file_path = self.clean_path(file_path);
    let mut files = self.files.lock();
    files.insert(file_path, Vec::from(bytes));
    Ok(())
  }

  fn remove_file(&self, file_path: impl AsRef<Path>) -> Result<(), ErrBox> {
    let file_path = self.clean_path(file_path);
    let mut files = self.files.lock();
    files.remove(&file_path);
    Ok(())
  }

  fn remove_dir_all(&self, dir_path: impl AsRef<Path>) -> Result<(), ErrBox> {
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

  fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox> {
    let remote_files = self.remote_files.lock();
    match remote_files.get(&String::from(url)) {
      Some(bytes) => Ok(bytes.clone()),
      None => err!("Could not find file at url {}", url),
    }
  }

  fn dir_info(&self, dir_path: impl AsRef<Path>) -> Result<Vec<DirEntry>, ErrBox> {
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

  fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, ErrBox> {
    Ok(self.clean_path(path))
  }

  fn is_absolute_path(&self, path: impl AsRef<Path>) -> bool {
    // cross platform check
    path.as_ref().to_string_lossy().starts_with("/") || path.as_ref().is_absolute()
  }

  fn mk_dir_all(&self, _: impl AsRef<Path>) -> Result<(), ErrBox> {
    Ok(())
  }

  fn cwd(&self) -> PathBuf {
    let cwd = self.cwd.lock();
    self.clean_path(PathBuf::from(cwd.to_owned()))
  }

  fn log(&self, text: &str) {
    if *self.is_silent.lock() {
      return;
    }
    self.logged_messages.lock().push(String::from(text));
  }

  fn log_error_with_context(&self, text: &str, _: &str) {
    if *self.is_silent.lock() {
      return;
    }
    self.logged_errors.lock().push(String::from(text));
  }

  fn log_silent(&self, text: &str) {
    self.logged_messages.lock().push(String::from(text));
  }

  fn log_action_with_progress<
    TResult: std::marker::Send + std::marker::Sync,
    TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
  >(
    &self,
    message: &str,
    action: TCreate,
    _: usize,
  ) -> TResult {
    self.log_error(message);
    action(Box::new(|_| {}))
  }

  fn get_cache_dir(&self) -> PathBuf {
    PathBuf::from("/cache")
  }

  fn get_time_secs(&self) -> u64 {
    123456
  }

  fn get_terminal_width(&self) -> u16 {
    60
  }

  fn get_selection(&self, prompt_message: &str, _: u16, _: &Vec<String>) -> Result<usize, ErrBox> {
    self.log_error(prompt_message);
    Ok(*self.selection_result.lock())
  }

  fn get_multi_selection(&self, prompt_message: &str, _: u16, _: &Vec<(bool, String)>) -> Result<Vec<usize>, ErrBox> {
    self.log_error(prompt_message);
    Ok(self.multi_selection_result.lock().clone())
  }

  fn confirm(&self, prompt_message: &str, default_value: bool) -> Result<bool, ErrBox> {
    let mut confirm_results = self.confirm_results.lock();
    let result = confirm_results.remove(0).map(|v| v.unwrap_or(default_value));
    self.log_error(&format!(
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

  fn compile_wasm(&self, _: &[u8]) -> Result<CompilationResult, ErrBox> {
    let wasm_compile_result = self.wasm_compile_result.lock();
    Ok(wasm_compile_result.clone().expect("Expected compilation result to be set."))
  }

  fn stdout(&self) -> Box<dyn Write + Send> {
    Box::new(self.std_out.clone())
  }

  fn stdin(&self) -> Box<dyn Read + Send> {
    Box::new(self.std_in.clone())
  }

  #[cfg(windows)]
  fn ensure_system_path(&self, directory_path: &str) -> Result<(), ErrBox> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if !path_dirs.contains(&directory_path) {
      path_dirs.push(directory_path);
    }
    Ok(())
  }

  #[cfg(windows)]
  fn remove_system_path(&self, directory_path: &str) -> Result<(), ErrBox> {
    let mut path_dirs = self.path_dirs.lock();
    let directory_path = PathBuf::from(directory_path);
    if let Some(pos) = path_dirs.iter().position(|p| p == &directory_path) {
      path_dirs.remove(pos);
    }
    Ok(())
  }
}
