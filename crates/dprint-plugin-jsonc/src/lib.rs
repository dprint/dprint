pub mod configuration;
mod format_text;
mod parser;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm_plugin;

pub use format_text::format_text;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use wasm_plugin::*;