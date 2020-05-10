extern crate dprint_core;

pub mod configuration;
mod parsing;
mod formatter;
mod plugin;
mod swc;
mod utils;

pub use formatter::Formatter;
pub use plugin::TypeScriptPlugin;

// Re-export swc for use in Deno
#[doc(hidden)]
pub use swc_common;
#[doc(hidden)]
pub use swc_ecma_ast;
#[doc(hidden)]
pub use swc_ecma_parser;
