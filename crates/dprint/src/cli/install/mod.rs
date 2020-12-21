#[cfg(target_os = "windows")]
mod windows_install;

pub use windows_install::*;
