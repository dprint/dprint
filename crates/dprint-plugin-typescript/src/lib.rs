extern crate dprint_core;

pub mod configuration;
mod parsing;
mod formatter;
mod swc;
mod utils;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm_plugin;

pub use formatter::Formatter;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use wasm_plugin::*;

// Re-export swc for use in Deno
#[doc(hidden)]
pub use swc_common;
#[doc(hidden)]
pub use swc_ecma_ast;
#[doc(hidden)]
pub use swc_ecma_parser;
