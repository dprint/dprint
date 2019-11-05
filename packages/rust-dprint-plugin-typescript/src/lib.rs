#[macro_use]
extern crate swc_common;
extern crate swc_ecma_parser;

mod format_text;
mod parse_to_swc_ast;
mod parser;

use parser::*;
use parse_to_swc_ast::*;

pub use format_text::{format_text};
