mod config;
mod editor;
mod formatting;
mod general;
mod lsp;
mod upgrade;
#[cfg(target_os = "windows")]
mod windows_install;

pub use config::*;
pub use editor::*;
pub use formatting::*;
pub use general::*;
pub use lsp::*;
pub use upgrade::*;
#[cfg(target_os = "windows")]
pub use windows_install::*;
