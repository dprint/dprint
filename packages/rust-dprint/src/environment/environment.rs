use std::path::PathBuf;

pub trait Environment : std::marker::Sync {
    fn read_file(&self, file_path: &PathBuf) -> Result<String, String>;
    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), String>;
    fn log(&self, text: &str);
    fn log_error(&self, text: &str);
}
