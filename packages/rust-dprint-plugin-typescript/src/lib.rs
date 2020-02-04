#[macro_use]
extern crate swc_common;
extern crate swc_ecma_parser;
extern crate dprint_core;

mod comments;
pub mod configuration;
mod node_helpers;
mod parser_types;
mod format_text;
mod parse_swc_ast;
mod parser;
mod tokens;
mod utils;

use comments::*;
use parser_types::*;
use parser::*;
use parse_swc_ast::*;
use tokens::*;

pub use format_text::{format_text};

#[cfg(test)]
mod configuration_tests;
