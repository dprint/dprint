use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use globset::{GlobSetBuilder, GlobSet, Glob};
use super::Environment;

pub struct TestEnvironment {
    files: Arc<Mutex<HashMap<PathBuf, String>>>,
    logged_messages: Arc<Mutex<Vec<String>>>,
    logged_errors: Arc<Mutex<Vec<String>>>,
}

impl TestEnvironment {
    pub fn new() -> TestEnvironment {
        TestEnvironment {
            files: Arc::new(Mutex::new(HashMap::new())),
            logged_messages: Arc::new(Mutex::new(Vec::new())),
            logged_errors: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl TestEnvironment {
    pub fn get_logged_messages(&self) -> Vec<String> {
        self.logged_messages.lock().unwrap().clone()
    }

    pub fn get_logged_errors(&self) -> Vec<String> {
        self.logged_errors.lock().unwrap().clone()
    }
}

impl Environment for TestEnvironment {
    fn read_file(&self, file_path: &PathBuf) -> Result<String, String> {
        let files = self.files.lock().unwrap();
        match files.get(file_path) {
            Some(text) => Ok(text.clone()),
            None => Err(format!("Could not find file at path {}", file_path.to_string_lossy())),
        }
    }

    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), String> {
        let mut files = self.files.lock().unwrap();
        files.insert(file_path.clone(), String::from(file_text));
        Ok(())
    }

    fn glob(&self, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, String> {
        let mut file_paths = Vec::new();
        let includes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| !p.starts_with("!")).map(|p| p.to_owned()))?;
        let excludes_set = file_patterns_to_glob_set(file_patterns.iter().filter(|p| p.starts_with("!")).map(|p| String::from(&p[1..])))?;
        let files = self.files.lock().unwrap();

        for key in files.keys() {
            if includes_set.is_match(key) && !excludes_set.is_match(key) {
                file_paths.push(key.clone());
            }
        }

        Ok(file_paths)
    }

    fn path_exists(&self, file_path: &PathBuf) -> bool {
        let files = self.files.lock().unwrap();
        files.contains_key(file_path)
    }

    fn log(&self, text: &str) {
        self.logged_messages.lock().unwrap().push(String::from(text));
    }

    fn log_error(&self, text: &str) {
        self.logged_errors.lock().unwrap().push(String::from(text));
    }
}

fn file_patterns_to_glob_set(file_patterns: impl Iterator<Item = String>) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for file_pattern in file_patterns {
        match Glob::new(&file_pattern) {
            Ok(glob) => { builder.add(glob); },
            Err(err) => return Err(format!("Error parsing glob {}: {}", file_pattern, err)),
        }
    }
    match builder.build() {
        Ok(glob_set) => Ok(glob_set),
        Err(err) => Err(format!("Error building glob set: {}", err)),
    }
}
