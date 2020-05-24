use std::path::PathBuf;
use std::collections::HashMap;

use crate::plugins::Plugin;

pub type FormatContexts = Vec<FormatContext>;

pub struct FormatContext {
    pub plugin: Box<dyn Plugin>,
    pub config: HashMap<String, String>,
    pub file_paths: Vec<PathBuf>,
}