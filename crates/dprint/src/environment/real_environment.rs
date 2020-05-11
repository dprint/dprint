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

    fn glob(&self, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, String> {
        let walker = match globwalk::GlobWalkerBuilder::from_patterns(&PathBuf::from("."), file_patterns).follow_links(true).build() {
            Ok(walker) => walker,
            Err(err) => return Err(format!("Error parsing file patterns: {}", err)),
        };

        let mut file_paths = Vec::new();
        for result in walker.into_iter() {
            match result {
                Ok(result) => { file_paths.push(result.into_path()); },
                Err(err) => return Err(format!("Error walking files: {}", err)),
            }
        }

        Ok(file_paths)
    }

    fn path_exists(&self, file_path: &PathBuf) -> bool {
        file_path.exists()
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
