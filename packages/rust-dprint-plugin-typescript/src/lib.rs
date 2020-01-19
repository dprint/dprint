#[macro_use]
extern crate swc_common;
extern crate swc_ecma_parser;

mod comments;
mod configuration;
mod node_helpers;
mod parser_types;
mod format_text;
mod parse_to_swc_ast;
mod parser;
mod tokens;
mod utils;

use comments::*;
use parser_types::*;
use parser::*;
use parse_to_swc_ast::*;
use tokens::*;

pub use configuration::*;
pub use format_text::{format_text};
