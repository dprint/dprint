use std::path::PathBuf;

use crate::plugins::Plugin;

pub type FormatContexts = Vec<FormatContext>;

pub struct FormatContext {
    pub plugin: Box<dyn Plugin>,
    pub file_paths: Vec<PathBuf>,
}