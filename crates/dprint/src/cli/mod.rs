mod arg_parser;
mod configuration;
mod editor_service;
mod format;
pub mod incremental;
#[cfg(target_os = "windows")]
mod install;
mod paths;
mod patterns;
mod plugins;
mod run_cli;
mod stdin_reader;

pub use arg_parser::*;
pub use run_cli::run_cli;
pub use stdin_reader::*;
