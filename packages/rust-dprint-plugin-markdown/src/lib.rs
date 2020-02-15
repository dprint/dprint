extern crate pulldown_cmark;
extern crate dprint_core;

mod ast_nodes;
pub mod configuration;
mod format_text;
mod parse_cmark_ast;
mod parsing;
mod parser;
mod parser_types;

pub use format_text::{format_text};
use parse_cmark_ast::*;

#[cfg(test)]
mod configuration_tests;
