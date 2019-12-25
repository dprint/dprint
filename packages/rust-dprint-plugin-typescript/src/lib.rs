#![feature(box_patterns)]

#[macro_use]
extern crate swc_common;
extern crate swc_ecma_parser;

mod configuration;
mod node_helpers;
mod parser_types;
mod format_text;
mod parse_to_swc_ast;
mod parser;
mod utils;

use parser_types::*;
use parser::*;
use parse_to_swc_ast::*;

pub use configuration::{TypeScriptConfiguration, resolve_config};
pub use format_text::{format_text};
