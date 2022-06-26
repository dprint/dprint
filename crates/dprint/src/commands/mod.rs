mod config;
mod editor;
mod formatting;
mod general;
mod upgrade;
#[cfg(target_os = "windows")]
mod windows_install;

pub use config::*;
pub use editor::*;
pub use formatting::*;
pub use general::*;
pub use upgrade::*;
#[cfg(target_os = "windows")]
pub use windows_install::*;
