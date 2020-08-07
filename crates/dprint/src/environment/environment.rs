use std::path::PathBuf;
use std::io::{Read, Write};
use async_trait::async_trait;
use bytes::Bytes;
use dprint_core::types::ErrBox;

use crate::plugins::CompilationResult;

#[async_trait]
pub trait Environment : Clone + std::marker::Send + std::marker::Sync + 'static {
    fn is_real(&self) -> bool;
    fn read_file(&self, file_path: &PathBuf) -> Result<String, ErrBox>;
    async fn read_file_async(&self, file_path: &PathBuf) -> Result<String, ErrBox>;
    fn read_file_bytes(&self, file_path: &PathBuf) -> Result<Bytes, ErrBox>;
    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox>;
    async fn write_file_async(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox>;
    fn write_file_bytes(&self, file_path: &PathBuf, bytes: &[u8]) -> Result<(), ErrBox>;
    fn remove_file(&self, file_path: &PathBuf) -> Result<(), ErrBox>;
    fn remove_dir_all(&self, dir_path: &PathBuf) -> Result<(), ErrBox>;
    fn glob(&self, base: &PathBuf, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox>;
    fn path_exists(&self, file_path: &PathBuf) -> bool;
    fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, ErrBox>;
    fn is_absolute_path(&self, path: &PathBuf) -> bool;
    fn mk_dir_all(&self, path: &PathBuf) -> Result<(), ErrBox>;
    fn cwd(&self) -> Result<PathBuf, ErrBox>;
    fn log(&self, text: &str);
    fn log_error(&self, text: &str);
    /// Information to output when logging is silent.
    fn log_silent(&self, text: &str);
    async fn log_action_with_progress<
        TResult: std::marker::Send + std::marker::Sync,
        TCreate : FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
    >(&self, message: &str, action: TCreate, total_size: usize) -> Result<TResult, ErrBox>;
    async fn download_file(&self, url: &str) -> Result<Bytes, ErrBox>;
    // async fn download_files(&self, urls: Vec<&str>) -> Result<Vec<Result<Bytes, ErrBox>>, ErrBox>;
    fn get_cache_dir(&self) -> Result<PathBuf, ErrBox>;
    fn get_time_secs(&self) -> u64;
    fn get_selection(&self, prompt_message: &str, items: &Vec<String>) -> Result<usize, ErrBox>;
    fn get_multi_selection(&self, prompt_message: &str, items: &Vec<String>) -> Result<Vec<usize>, ErrBox>;
    fn is_verbose(&self) -> bool;
    fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox>;
    fn stdout(&self) -> Box<dyn Write + Send>;
    fn stdin(&self) -> Box<dyn Read + Send>;
}

// use a macro here so the expression provided is only evaluated when in verbose mode
macro_rules! log_verbose {
    ($environment:expr, $($arg:tt)*) => {
        if $environment.is_verbose() {
            let mut text = String::from("[VERBOSE]: ");
            text.push_str(&format!($($arg)*));
            $environment.log(&text);
        }
    }
}
