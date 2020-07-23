use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use globset::{GlobSetBuilder, GlobSet, Glob};
use async_trait::async_trait;
use bytes::Bytes;
use path_clean::{PathClean};

use super::Environment;
use crate::types::ErrBox;
use crate::plugins::CompilationResult;

#[derive(Clone)]
pub struct TestEnvironment {
    is_verbose: Arc<Mutex<bool>>,
    cwd: Arc<Mutex<String>>,
    files: Arc<Mutex<HashMap<PathBuf, Bytes>>>,
    logged_messages: Arc<Mutex<Vec<String>>>,
    logged_errors: Arc<Mutex<Vec<String>>>,
    remote_files: Arc<Mutex<HashMap<String, Bytes>>>,
    deleted_directories: Arc<Mutex<Vec<PathBuf>>>,
    selection_result: Arc<Mutex<usize>>,
    multi_selection_result: Arc<Mutex<Vec<usize>>>,
    is_silent: Arc<Mutex<bool>>,
    wasm_compile_result: Arc<Mutex<Option<CompilationResult>>>,
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
        }
    }

    pub fn get_logged_messages(&self) -> Vec<String> {
        self.logged_messages.lock().unwrap().clone()
    }

    pub fn clear_logs(&self) {
        self.logged_messages.lock().unwrap().clear();
        self.logged_errors.lock().unwrap().clear();
    }

    pub fn get_logged_errors(&self) -> Vec<String> {
        self.logged_errors.lock().unwrap().clone()
    }

    pub fn add_remote_file(&self, path: &str, bytes: &'static [u8]) {
        self.add_remote_file_bytes(path, Bytes::from(bytes));
    }

    pub fn add_remote_file_bytes(&self, path: &str, bytes: Bytes) {
        let mut remote_files = self.remote_files.lock().unwrap();
        remote_files.insert(String::from(path), bytes);
    }

    pub fn is_dir_deleted(&self, path: &PathBuf) -> bool {
        let deleted_directories = self.deleted_directories.lock().unwrap();
        deleted_directories.contains(path)
    }

    pub fn set_selection_result(&self, index: usize) {
        let mut selection_result = self.selection_result.lock().unwrap();
        *selection_result = index;
    }

    pub fn set_multi_selection_result(&self, indexes: Vec<usize>) {
        let mut multi_selection_result = self.multi_selection_result.lock().unwrap();
        *multi_selection_result = indexes;
    }

    pub fn set_cwd(&self, new_path: &str) {
        let mut cwd = self.cwd.lock().unwrap();
        *cwd = String::from(new_path);
    }

    pub fn set_silent(&self, value: bool) {
        let mut is_silent = self.is_silent.lock().unwrap();
        *is_silent = value;
    }

    pub fn set_verbose(&self, value: bool) {
        let mut is_verbose = self.is_verbose.lock().unwrap();
        *is_verbose = value;
    }

    pub fn set_wasm_compile_result(&self, value: CompilationResult) {
        let mut wasm_compile_result = self.wasm_compile_result.lock().unwrap();
        *wasm_compile_result = Some(value);
    }
}

#[async_trait]
impl Environment for TestEnvironment {
    fn is_real(&self) -> bool {
        false
    }

    fn read_file(&self, file_path: &PathBuf) -> Result<String, ErrBox> {
        let file_bytes = self.read_file_bytes(file_path)?;
        Ok(String::from_utf8(file_bytes.to_vec()).unwrap())
    }

    async fn read_file_async(&self, file_path: &PathBuf) -> Result<String, ErrBox> {
        self.read_file(file_path)
    }

    fn read_file_bytes(&self, file_path: &PathBuf) -> Result<Bytes, ErrBox> {
        let files = self.files.lock().unwrap();
        // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
        let file_path = PathBuf::from(file_path.to_string_lossy().replace("\\", "/"));
        match files.get(&file_path.clean()) {
            Some(text) => Ok(text.clone()),
            None => err!("Could not find file at path {}", file_path.display()),
        }
    }

    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox> {
        self.write_file_bytes(file_path, file_text.as_bytes())
    }

    async fn write_file_async(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox> {
        self.write_file(file_path, file_text)
    }

    fn write_file_bytes(&self, file_path: &PathBuf, bytes: &[u8]) -> Result<(), ErrBox> {
        let mut files = self.files.lock().unwrap();
        files.insert(file_path.clean(), Bytes::from(bytes.to_vec()));
        Ok(())
    }

    fn remove_file(&self, file_path: &PathBuf) -> Result<(), ErrBox> {
        let mut files = self.files.lock().unwrap();
        files.remove(&file_path.clean());
        Ok(())
    }

    fn remove_dir_all(&self, dir_path: &PathBuf) -> Result<(), ErrBox> {
        let mut deleted_directories = self.deleted_directories.lock().unwrap();
        deleted_directories.push(dir_path.to_owned());
        Ok(())
    }

    async fn download_file(&self, url: &str) -> Result<Bytes, ErrBox> {
        let remote_files = self.remote_files.lock().unwrap();
        match remote_files.get(&String::from(url)) {
            Some(bytes) => Ok(bytes.clone()),
            None => err!("Could not find file at url {}", url),
        }
    }

    fn glob(&self, _: &PathBuf, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
        // todo: would be nice to test the base parameter here somehow...
        let mut file_paths = Vec::new();
        let includes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| !p.starts_with("!")).map(|p| p.to_owned()))?;
        let excludes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| p.starts_with("!")).map(|p| String::from(&p[1..])))?;
        let files = self.files.lock().unwrap();

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

    fn path_exists(&self, file_path: &PathBuf) -> bool {
        let files = self.files.lock().unwrap();
        files.contains_key(&file_path.clean())
    }

    fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, ErrBox> {
        // temporary until https://github.com/danreeves/path-clean/issues/4 is fixed in path-clean
        let file_path = PathBuf::from(path.to_string_lossy().replace("\\", "/"));
        Ok(file_path.clean())
    }

    fn is_absolute_path(&self, path: &PathBuf) -> bool {
        path.to_string_lossy().starts_with("/")
    }

    fn mk_dir_all(&self, _: &PathBuf) -> Result<(), ErrBox> {
        Ok(())
    }

    fn cwd(&self) -> Result<PathBuf, ErrBox> {
        let cwd = self.cwd.lock().unwrap();
        Ok(PathBuf::from(cwd.to_owned()))
    }

    fn log(&self, text: &str) {
        if *self.is_silent.lock().unwrap() { return; }
        self.logged_messages.lock().unwrap().push(String::from(text));
    }

    fn log_error(&self, text: &str) {
        if *self.is_silent.lock().unwrap() { return; }
        self.logged_errors.lock().unwrap().push(String::from(text));
    }

    fn log_silent(&self, text: &str) {
        self.logged_messages.lock().unwrap().push(String::from(text));
    }

    async fn log_action_with_progress<
        TResult: std::marker::Send + std::marker::Sync,
        TCreate : FnOnce() -> TResult + std::marker::Send + std::marker::Sync
    >(&self, message: &str, action: TCreate) -> Result<TResult, ErrBox> {
        self.log(message);
        Ok(action())
    }

    fn get_cache_dir(&self) -> Result<PathBuf, ErrBox> {
        Ok(PathBuf::from("/cache"))
    }

    fn get_time_secs(&self) -> u64 {
        123456
    }

    fn get_selection(&self, prompt_message: &str, _: &Vec<String>) -> Result<usize, ErrBox> {
        self.log(prompt_message);
        Ok(*self.selection_result.lock().unwrap())
    }

    fn get_multi_selection(&self, prompt_message: &str, _: &Vec<String>) -> Result<Vec<usize>, ErrBox> {
        self.log(prompt_message);
        Ok(self.multi_selection_result.lock().unwrap().clone())
    }

    fn is_verbose(&self) -> bool {
        *self.is_verbose.lock().unwrap()
    }

    fn compile_wasm(&self, _: &[u8]) -> Result<CompilationResult, ErrBox> {
        let wasm_compile_result = self.wasm_compile_result.lock().unwrap();
        Ok(wasm_compile_result.clone().expect("Expected compilation result to be set."))
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
