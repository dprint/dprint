use std::path::PathBuf;
use std::time::SystemTime;
use std::fs;
use async_trait::async_trait;
use dprint_core::types::ErrBox;
use dprint_cli_core::{download_url};
use dprint_cli_core::logging::{Logger, ProgressBars, log_action_with_progress};

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
        let progress_bars = ProgressBars::new(&logger);
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

#[async_trait]
impl Environment for RealEnvironment {
    fn is_real(&self) -> bool {
        true
    }

    fn read_file(&self, file_path: &PathBuf) -> Result<String, ErrBox> {
        Ok(String::from_utf8(self.read_file_bytes(file_path)?)?)
    }

    fn read_file_bytes(&self, file_path: &PathBuf) -> Result<Vec<u8>, ErrBox> {
        log_verbose!(self, "Reading file: {}", file_path.display());
        Ok(fs::read(file_path)?)
    }

    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox> {
        log_verbose!(self, "Writing file: {}", file_path.display());
        fs::write(file_path, file_text)?;
        Ok(())
    }

    fn write_file_bytes(&self, file_path: &PathBuf, bytes: &[u8]) -> Result<(), ErrBox> {
        log_verbose!(self, "Writing file: {}", file_path.display());
        fs::write(file_path, bytes)?;
        Ok(())
    }

    fn remove_file(&self, file_path: &PathBuf) -> Result<(), ErrBox> {
        log_verbose!(self, "Deleting file: {}", file_path.display());
        fs::remove_file(file_path)?;
        Ok(())
    }

    fn remove_dir_all(&self, dir_path: &PathBuf) -> Result<(), ErrBox> {
        log_verbose!(self, "Deleting directory: {}", dir_path.display());
        fs::remove_dir_all(dir_path)?;
        Ok(())
    }

    async fn download_file(&self, url: &str) -> Result<Vec<u8>, ErrBox> {
        log_verbose!(self, "Downloading url: {}", url);

        download_url(url, &self.progress_bars).await
    }

    fn glob(&self, base: &PathBuf, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
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

    fn path_exists(&self, file_path: &PathBuf) -> bool {
        log_verbose!(self, "Checking path exists: {}", file_path.display());
        file_path.exists()
    }

    fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, ErrBox> {
        // use this to avoid //?//C:/etc... like paths on windows (UNC)
        Ok(dunce::canonicalize(path)?)
    }

    fn is_absolute_path(&self, path: &PathBuf) -> bool {
        path.is_absolute()
    }

    fn mk_dir_all(&self, path: &PathBuf) -> Result<(), ErrBox> {
        fs::create_dir_all(path)?;
        Ok(())
    }

    fn cwd(&self) -> Result<PathBuf, ErrBox> {
        Ok(std::env::current_dir()?)
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

    fn get_selection(&self, prompt_message: &str, items: &Vec<String>) -> Result<usize, ErrBox> {
        use dialoguer::*;

        let result = Select::with_theme(&CustomDialoguerTheme::new())
            .with_prompt(prompt_message)
            .items(items)
            .default(0)
            .interact()?;
        Ok(result)
    }

    fn get_multi_selection(&self, prompt_message: &str, items: &Vec<String>) -> Result<Vec<usize>, ErrBox> {
        use dialoguer::*;
        let result = MultiSelect::with_theme(&CustomDialoguerTheme::new())
            .with_prompt(prompt_message)
            .items_checked(&items.iter().map(|item| (item, true)).collect::<Vec<_>>())
            .interact()?;
        Ok(result)
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

struct CustomDialoguerTheme {
}

impl CustomDialoguerTheme {
    pub fn new() -> Self {
        CustomDialoguerTheme {}
    }
}

impl dialoguer::theme::Theme for CustomDialoguerTheme {
    #[inline]
    fn format_prompt(&self, f: &mut dyn std::fmt::Write, prompt: &str) -> std::fmt::Result {
        // render without colon
        write!(f, "{}", prompt)
    }

    #[inline]
    fn format_input_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> std::fmt::Result {
        write!(f, "{}\n  {}", prompt, sel)
    }

    fn format_multi_select_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> std::fmt::Result {
        write!(f, "{}", prompt)?;
        for sel in selections.iter() {
            write!(f, "\n  * {}", sel)?;
        }
        Ok(())
    }
}
