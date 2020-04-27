use std::path::PathBuf;
use std::fs;
use super::Environment;
use std::sync::{Arc, Mutex};

pub struct RealEnvironment {
    output_lock: Arc<Mutex<u8>>,
}

impl RealEnvironment {
    pub fn new() -> RealEnvironment {
        RealEnvironment { output_lock: Arc::new(Mutex::new(0)), }
    }
}

impl Environment for RealEnvironment {
    fn read_file(&self, file_path: &PathBuf) -> Result<String, String> {
        match fs::read_to_string(file_path) {
            Ok(text) => Ok(text),
            Err(err) => Err(err.to_string()),
        }
    }

    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), String> {
        match fs::write(file_path, file_text) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    fn log(&self, text: &str) {
        let _g = self.output_lock.lock().unwrap();
        println!("{}", text);
    }

    fn log_error(&self, text: &str) {
        let _g = self.output_lock.lock().unwrap();
        eprintln!("{}", text);
    }
}
