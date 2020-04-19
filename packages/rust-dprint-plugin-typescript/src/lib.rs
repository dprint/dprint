extern crate dprint_core;

mod comments;
pub mod configuration;
mod node_helpers;
mod parser_types;
mod formatter;
mod parse_swc_ast;
mod parser;
mod tokens;
mod helpers;
mod utils;

use comments::*;
use parser_types::*;
use parser::*;
use parse_swc_ast::*;
use tokens::*;

pub use formatter::Formatter;

#[cfg(test)]
mod configuration_tests;

// Re-export swc for use in Deno
#[doc(hidden)]
pub use swc_common;
#[doc(hidden)]
pub use swc_ecma_ast;
#[doc(hidden)]
pub use swc_ecma_parser;
