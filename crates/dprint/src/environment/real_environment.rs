use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::io::ErrorKind;
use std::fs;
use dprint_core::types::ErrBox;
use dprint_cli_core::{download_url};
use dprint_cli_core::logging::{Logger, ProgressBars, log_action_with_progress, show_select, show_multi_select};

use super::Environment;
use crate::plugins::CompilationResult;

#[derive(Clone)]
pub struct RealEnvironment {
    logger: Logger,
    progress_bars: Option<ProgressBars>,
    is_verbose: bool,
}

impl RealEnvironment {
    pub fn new(is_verbose: bool, is_silent: bool) -> Result<RealEnvironment, ErrBox> {
        let logger = Logger::new("dprint", is_silent);
        let progress_bars = if is_silent {
            None
        } else {
            ProgressBars::new(&logger)
        };
        let environment = RealEnvironment {
            logger,
            progress_bars,
            is_verbose,
        };

        // ensure the cache directory is created
        if let Err(err) = environment.mk_dir_all(&get_cache_dir()?) {
            return err!("Error creating cache directory: {:?}", err);
        }

        Ok(environment)
    }
}

impl Environment for RealEnvironment {
    fn is_real(&self) -> bool {
        true
    }

    fn read_file(&self, file_path: &Path) -> Result<String, ErrBox> {
        Ok(String::from_utf8(self.read_file_bytes(file_path)?)?)
    }

    fn read_file_bytes(&self, file_path: &Path) -> Result<Vec<u8>, ErrBox> {
        log_verbose!(self, "Reading file: {}", file_path.display());
        match fs::read(file_path) {
            Ok(bytes) => Ok(bytes),
            Err(err) => err!("Error reading file {}: {}", file_path.display(), err.to_string()),
        }
    }

    fn write_file(&self, file_path: &Path, file_text: &str) -> Result<(), ErrBox> {
        self.write_file_bytes(file_path, file_text.as_bytes())
    }

    fn write_file_bytes(&self, file_path: &Path, bytes: &[u8]) -> Result<(), ErrBox> {
        log_verbose!(self, "Writing file: {}", file_path.display());
        match fs::write(file_path, bytes) {
            Ok(_) => Ok(()),
            Err(err) => err!("Error writing file {}: {}", file_path.display(), err.to_string()),
        }
    }

    fn remove_file(&self, file_path: &Path) -> Result<(), ErrBox> {
        log_verbose!(self, "Deleting file: {}", file_path.display());
        match fs::remove_file(file_path) {
            Ok(_) => Ok(()),
            Err(err) => err!("Error deleting file {}: {}", file_path.display(), err.to_string()),
        }
    }

    fn remove_dir_all(&self, dir_path: &Path) -> Result<(), ErrBox> {
        log_verbose!(self, "Deleting directory: {}", dir_path.display());
        match fs::remove_dir_all(dir_path) {
            Ok(_) => Ok(()),
            Err(err) => {
                if err.kind() == ErrorKind::NotFound {
                    Ok(())
                } else {
                    err!("Error removing directory {}: {}", dir_path.display(), err.to_string())
                }
            }
        }
    }

    fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox> {
        log_verbose!(self, "Downloading url: {}", url);

        download_url(url, &self.progress_bars)
    }

    fn glob(&self, base: &Path, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
        let start_instant = std::time::Instant::now();
        log_verbose!(self, "Globbing: {:?}", file_patterns);
        let base = self.canonicalize(base)?;
        let walker = globwalk::GlobWalkerBuilder::from_patterns(base, file_patterns)
            .follow_links(false)
            .file_type(globwalk::FileType::FILE)
            .build();
        let walker = match walker {
            Ok(walker) => walker,
            Err(err) => return err!("Error parsing file patterns: {}", err),
        };

        let mut file_paths = Vec::new();
        for result in walker.into_iter() {
            match result {
                Ok(result) => file_paths.push(result.into_path()),
                Err(err) => return err!("Error walking files: {}", err),
            }
        }

        log_verbose!(self, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

        Ok(file_paths)
    }

    fn path_exists(&self, file_path: &Path) -> bool {
        log_verbose!(self, "Checking path exists: {}", file_path.display());
        file_path.exists()
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, ErrBox> {
        // use this to avoid //?//C:/etc... like paths on windows (UNC)
        Ok(dunce::canonicalize(path)?)
    }

    fn is_absolute_path(&self, path: &Path) -> bool {
        path.is_absolute()
    }

    fn mk_dir_all(&self, path: &Path) -> Result<(), ErrBox> {
        log_verbose!(self, "Creating directory: {}", path.display());
        match fs::create_dir_all(path) {
            Ok(_) => Ok(()),
            Err(err) => err!("Error creating directory {}: {}", path.display(), err.to_string()),
        }
    }

    fn cwd(&self) -> Result<PathBuf, ErrBox> {
        match std::env::current_dir() {
            Ok(cwd) => Ok(cwd),
            Err(err) => err!("Error getting current working: {}", err.to_string()),
        }
    }

    fn log(&self, text: &str) {
        self.logger.log(text, "dprint");
    }

    fn log_silent(&self, text: &str) {
        self.logger.log_bypass_silent(text, "dprint");
    }

    fn log_error_with_context(&self, text: &str, context_name: &str) {
        self.logger.log_err(text, context_name);
    }

    fn log_action_with_progress<
        TResult: std::marker::Send + std::marker::Sync,
        TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
    >(&self, message: &str, action: TCreate, total_size: usize) -> TResult {
        log_action_with_progress(&self.progress_bars, message, action, total_size)
    }

    fn get_cache_dir(&self) -> PathBuf {
        // this would have errored in the constructor so it's ok to unwrap here
        get_cache_dir().unwrap()
    }

    fn get_time_secs(&self) -> u64 {
        SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
    }

    fn get_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<String>) -> Result<usize, ErrBox> {
        show_select(
            &self.logger,
            "dprint",
            prompt_message,
            item_indent_width,
            items
        )
    }

    fn get_multi_selection(&self, prompt_message: &str, item_indent_width: u16, items: &Vec<(bool, String)>) -> Result<Vec<usize>, ErrBox> {
        show_multi_select(
            &self.logger,
            "dprint",
            prompt_message,
            item_indent_width,
            items.iter().map(|(value, text)| (value.to_owned(), text)).collect(),
        )
    }

    fn get_terminal_width(&self) -> u16 {
        dprint_cli_core::terminal::get_terminal_width().unwrap_or(60)
    }

    #[inline]
    fn is_verbose(&self) -> bool {
        self.is_verbose
    }

    fn compile_wasm(&self, wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
        crate::plugins::compile_wasm(wasm_bytes)
    }

    fn stdout(&self) -> Box<dyn std::io::Write + Send> {
        Box::new(std::io::stdout())
    }

    fn stdin(&self) -> Box<dyn std::io::Read + Send> {
        Box::new(std::io::stdin())
    }
}

fn get_cache_dir() -> Result<PathBuf, ErrBox> {
    match dirs::cache_dir() {
        Some(dir) => Ok(dir.join("dprint").join("cache")),
        None => err!("Expected to find cache directory")
    }
}
