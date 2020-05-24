use std::path::PathBuf;
use async_trait::async_trait;
use bytes::Bytes;
use super::super::types::ErrBox;

#[async_trait]
pub trait Environment : Clone + std::marker::Send + 'static {
    fn read_file(&self, file_path: &PathBuf) -> Result<String, ErrBox>;
    fn read_file_bytes(&self, file_path: &PathBuf) -> Result<Bytes, ErrBox>;
    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox>;
    fn write_file_bytes(&self, file_path: &PathBuf, bytes: &[u8]) -> Result<(), ErrBox>;
    fn remove_file(&self, file_path: &PathBuf) -> Result<(), ErrBox>;
    fn remove_dir_all(&self, dir_path: &PathBuf) -> Result<(), ErrBox>;
    fn glob(&self, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox>;
    fn path_exists(&self, file_path: &PathBuf) -> bool;
    fn log(&self, text: &str);
    fn log_error(&self, text: &str);
    async fn download_file(&self, url: &str) -> Result<Bytes, ErrBox>;
    fn get_cache_dir(&self) -> Result<PathBuf, ErrBox>;
    fn get_time_secs(&self) -> u64;
    fn get_selection(&self, items: &Vec<String>) -> Result<usize, ErrBox>;
}
