mod config;
mod editor;
mod formatting;
mod general;
#[cfg(target_os = "windows")]
mod windows_install;

pub use config::*;
pub use editor::*;
pub use formatting::*;
pub use general::*;
#[cfg(target_os = "windows")]
pub use windows_install::*;
