use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::io::{Read, Write, Error};
use async_trait::async_trait;
use globset::{GlobSetBuilder, GlobSet, Glob};
use parking_lot::Mutex;
use path_clean::{PathClean};
use dprint_core::types::ErrBox;

use super::Environment;
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
            buffer_data: Arc::new(Mutex::new(BufferData {
                data: Vec::new(),
                read_pos: 0,
            })),
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
    is_silent: Arc<Mutex<bool>>,
    wasm_compile_result: Arc<Mutex<Option<CompilationResult>>>,
    std_in: MockStdInOut,
    std_out: MockStdInOut,
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
            is_silent: Arc::new(Mutex::new(false)),
            wasm_compile_result: Arc::new(Mutex::new(None)),
            std_in: MockStdInOut::new(),
            std_out: MockStdInOut::new(),
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

    pub fn is_dir_deleted(&self, path: &Path) -> bool {
        let deleted_directories = self.deleted_directories.lock();
        deleted_directories.contains(&path.to_path_buf())
    }

    pub fn set_selection_result(&self, index: usize) {
        let mut selection_result = self.selection_result.lock();
        *selection_result = index;
    }

    pub fn set_multi_selection_result(&self, indexes: Vec<usize>) {
        let mut multi_selection_result = self.multi_selection_result.lock();
        *multi_selection_result = indexes;
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
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // If this panics that means the logged messages or errors weren't inspected for a test.
        // Use take_logged_messages() or take_logged_errors() and inspect the results.
        if !std::thread::panicking() && Arc::strong_count(&self.logged_messages) == 1 {
            assert_eq!(self.logged_messages.lock().clone(), Vec::<String>::new(), "should not have logged messages left on drop");
            assert_eq!(self.logged_errors.lock().clone(), Vec::<String>::new(), "should not have logged errors left on drop");
        }
    }
}

#[async_trait]
impl Environment for TestEnvironment {
    fn is_real(&self) -> bool {
        false
    }

    fn read_file(&self, file_path: &Path) -> Result<String, ErrBox> {
        let file_bytes = self.read_file_bytes(file_path)?;
        Ok(String::from_utf8(file_bytes.to_vec()).unwrap())
    }

    fn read_file_bytes(&self, file_path: &Path) -> Result<Vec<u8>, ErrBox> {
        let files = self.files.lock();
        // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
        let file_path = PathBuf::from(file_path.to_string_lossy().replace("\\", "/"));
        match files.get(&file_path.clean()) {
            Some(text) => Ok(text.clone()),
            None => err!("Could not find file at path {}", file_path.display()),
        }
    }

    fn write_file(&self, file_path: &Path, file_text: &str) -> Result<(), ErrBox> {
        self.write_file_bytes(file_path, file_text.as_bytes())
    }

    fn write_file_bytes(&self, file_path: &Path, bytes: &[u8]) -> Result<(), ErrBox> {
        let mut files = self.files.lock();
        files.insert(file_path.to_path_buf().clean(), Vec::from(bytes));
        Ok(())
    }

    fn remove_file(&self, file_path: &Path) -> Result<(), ErrBox> {
        let mut files = self.files.lock();
        files.remove(&file_path.to_path_buf().clean());
        Ok(())
    }

    fn remove_dir_all(&self, dir_path: &Path) -> Result<(), ErrBox> {
        {
            let mut deleted_directories = self.deleted_directories.lock();
            deleted_directories.push(dir_path.to_owned());
        }
        let dir_path = dir_path.to_path_buf().clean();
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

    async fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox> {
        let remote_files = self.remote_files.lock();
        match remote_files.get(&String::from(url)) {
            Some(bytes) => Ok(bytes.clone()),
            None => err!("Could not find file at url {}", url),
        }
    }

    fn glob(&self, _: &Path, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
        // todo: would be nice to test the base parameter here somehow...
        let mut file_paths = Vec::new();
        let includes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| !p.starts_with("!")).map(|p| p.to_owned()))?;
        let excludes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| p.starts_with("!")).map(|p| String::from(&p[1..])))?;
        let files = self.files.lock();

        for key in files.keys() {
            let mut has_exclude = false;
            if excludes_set.is_match(key.file_name().unwrap()) {
                has_exclude = true;
            } else {
                for ancestor in key.ancestors() {
                    if excludes_set.is_match(ancestor) {
                        has_exclude = true;
                        break;
                    }
                }
            }

            if !has_exclude {
                if includes_set.is_match(key) || includes_set.is_match(key.file_name().unwrap()) {
                    file_paths.push(key.clone());
                }
            }
        }

        Ok(file_paths)
    }

    fn path_exists(&self, file_path: &Path) -> bool {
        let files = self.files.lock();
        files.contains_key(&file_path.to_path_buf().clean())
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, ErrBox> {
        // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
        let file_path = PathBuf::from(path.to_string_lossy().replace("\\", "/"));
        Ok(file_path.clean())
    }

    fn is_absolute_path(&self, path: &Path) -> bool {
        path.to_string_lossy().starts_with("/")
    }

    fn mk_dir_all(&self, _: &Path) -> Result<(), ErrBox> {
        Ok(())
    }

    fn cwd(&self) -> Result<PathBuf, ErrBox> {
        let cwd = self.cwd.lock();
        Ok(PathBuf::from(cwd.to_owned()))
    }

    fn log(&self, text: &str) {
        if *self.is_silent.lock() { return; }
        self.logged_messages.lock().push(String::from(text));
    }

    fn log_error_with_context(&self, text: &str, _: &str) {
        if *self.is_silent.lock() { return; }
        self.logged_errors.lock().push(String::from(text));
    }

    fn log_silent(&self, text: &str) {
        self.logged_messages.lock().push(String::from(text));
    }

    fn log_action_with_progress<
        TResult: std::marker::Send + std::marker::Sync,
        TCreate : FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync
    >(&self, message: &str, action: TCreate, _: usize) -> TResult {
        self.log_error(message);
        action(Box::new(|_| {}))
    }

    fn get_cache_dir(&self) -> PathBuf {
        PathBuf::from("/cache")
    }

    fn get_time_secs(&self) -> u64 {
        123456
    }

    fn get_selection(&self, prompt_message: &str, _: &Vec<String>) -> Result<usize, ErrBox> {
        self.log_error(prompt_message);
        Ok(*self.selection_result.lock())
    }

    fn get_multi_selection(&self, prompt_message: &str, _: &Vec<String>) -> Result<Vec<usize>, ErrBox> {
        self.log_error(prompt_message);
        Ok(self.multi_selection_result.lock().clone())
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
}

fn file_patterns_to_glob_set(file_patterns: impl Iterator<Item = String>) -> Result<GlobSet, ErrBox> {
    let mut builder = GlobSetBuilder::new();
    for file_pattern in file_patterns {
        match Glob::new(&file_pattern) {
            Ok(glob) => { builder.add(glob); },
            Err(err) => return err!("Error parsing glob {}: {}", file_pattern, err),
        }
    }
    return match builder.build() {
        Ok(glob_set) => Ok(glob_set),
        Err(err) => err!("Error building glob set: {}", err),
    };
}
