use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

    fn log(&self, text: &str) {
        self.logged_messages.lock().unwrap().push(String::from(text));
    }

    fn log_error(&self, text: &str) {
        self.logged_errors.lock().unwrap().push(String::from(text));
    }
}
