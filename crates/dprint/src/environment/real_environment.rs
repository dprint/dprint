use std::path::PathBuf;
use std::time::SystemTime;
use std::fs;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client;
use dprint_core::types::ErrBox;

use super::{Environment, ProgressBars, ProgressBarStyle};
use crate::plugins::CompilationResult;

#[derive(Clone)]
pub struct RealEnvironment {
    output_lock: Arc<Mutex<u8>>,
    progress_bars: Arc<ProgressBars>,
    is_verbose: bool,
    is_silent: bool,
}

impl RealEnvironment {
    pub fn new(is_verbose: bool, is_silent: bool) -> RealEnvironment {
        RealEnvironment {
            output_lock: Arc::new(Mutex::new(0)),
            progress_bars: Arc::new(ProgressBars::new()),
            is_verbose,
            is_silent,
        }
    }
}

const APP_INFO: app_dirs::AppInfo = app_dirs::AppInfo { name: "dprint", author: "dprint" };

#[async_trait]
impl Environment for RealEnvironment {
    fn is_real(&self) -> bool {
        true
    }

    fn read_file(&self, file_path: &PathBuf) -> Result<String, ErrBox> {
        log_verbose!(self, "Reading file: {}", file_path.display());
        let text = fs::read_to_string(file_path)?;
        Ok(text)
    }

    async fn read_file_async(&self, file_path: &PathBuf) -> Result<String, ErrBox> {
        log_verbose!(self, "Reading file: {}", file_path.display());
        let text = tokio::fs::read_to_string(file_path).await?;
        Ok(text)
    }

    fn read_file_bytes(&self, file_path: &PathBuf) -> Result<Bytes, ErrBox> {
        log_verbose!(self, "Reading file: {}", file_path.display());
        let bytes = fs::read(file_path)?;
        Ok(Bytes::from(bytes))
    }

    fn write_file(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox> {
        log_verbose!(self, "Writing file: {}", file_path.display());
        fs::write(file_path, file_text)?;
        Ok(())
    }

    async fn write_file_async(&self, file_path: &PathBuf, file_text: &str) -> Result<(), ErrBox> {
        log_verbose!(self, "Writing file: {}", file_path.display());
        tokio::fs::write(file_path, file_text).await?;
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

    async fn download_file(&self, url: &str) -> Result<Bytes, ErrBox> {
        log_verbose!(self, "Downloading url: {}", url);

        let client = Client::new();
        let mut resp = client.get(url).send().await?;
        let total_size = {
            if resp.status().is_success() {
                resp.content_length()
            } else {
                return err!("Error downloading: {}. Status: {:?}", url, resp.status());
            }
        }.unwrap_or(0);

        if self.is_silent {
            Ok(resp.bytes().await?)
        } else {
            let message = get_middle_truncted_text("Downloading ", url);
            let pb = self.progress_bars.add_progress(&message, ProgressBarStyle::Download, total_size);
            let mut final_bytes = bytes::BytesMut::with_capacity(total_size as usize);

            while let Some(chunk) = resp.chunk().await? {
                final_bytes.extend_from_slice(&chunk);
                pb.set_position(final_bytes.len() as u64);
            }

            pb.finish_and_clear();
            self.progress_bars.finish_one().await?;

            Ok(final_bytes.freeze())
        }
    }

    fn glob(&self, base: &PathBuf, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
        let start_instant = std::time::Instant::now();
        log_verbose!(self, "Globbing: {:?}", file_patterns);
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
                Ok(result) => { file_paths.push(result.into_path()); },
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
        if self.is_silent { return; }
        let _g = self.output_lock.lock().unwrap();
        println!("{}", text);
    }

    fn log_silent(&self, text: &str) {
        let _g = self.output_lock.lock().unwrap();
        println!("{}", text);
    }

    async fn log_action_with_progress<
        TResult: std::marker::Send + std::marker::Sync,
        TCreate : FnOnce() -> TResult + std::marker::Send + std::marker::Sync
    >(&self, message: &str, action: TCreate) -> Result<TResult, ErrBox> {
        let pb = self.progress_bars.add_progress(message, ProgressBarStyle::Action, 1);
        let result = action();
        pb.finish_and_clear();
        self.progress_bars.finish_one().await?;
        Ok(result)
    }

    fn log_error(&self, text: &str) {
        if self.is_silent { return; }
        let _g = self.output_lock.lock().unwrap();
        eprintln!("{}", text);
    }

    fn get_cache_dir(&self) -> Result<PathBuf, ErrBox> {
        log_verbose!(self, "Getting cache directory.");
        match app_dirs::app_dir(app_dirs::AppDataType::UserCache, &APP_INFO, "cache") {
            Ok(path) => Ok(path),
            Err(err) => err!("Error getting cache directory: {:?}", err),
        }
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

/// This is necessary because Indicatif only supports messages on one line. If the lines span
/// multiple lines then issue #278 occurs.
///
/// This takes a text like "Downloading " and "https://dprint.dev/somelongurl"
/// and may truncate it to "Downloading https://dprint.dev...longurl"
fn get_middle_truncted_text(prompt: &str, text: &str) -> String {
    // For some reason, the "console" crate was not correctly returning
    // the terminal size, so ended up using the terminal_size crate directly
    use terminal_size::{Width, terminal_size};

    let term_width = if let Some((Width(width), _)) = terminal_size() {
        width as usize
    } else {
        return format!("{}{}", prompt, text);
    };

    let prompt_width = console::measure_text_width(prompt);
    let text_width = console::measure_text_width(text);

    if prompt_width + text_width < term_width {
        format!("{}{}", prompt, text)
    } else {
        let middle_point = (term_width - prompt_width) / 2;
        let text_chars = text.chars().collect::<Vec<_>>();
        let first_text: String = (&text_chars[0..middle_point - 2]).iter().collect();
        let second_text: String = (&text_chars[text_chars.len() - middle_point + 1..]).iter().collect();
        let text = format!("{}...{}", first_text, second_text);
        format!("{}{}", prompt, text)
    }
}
