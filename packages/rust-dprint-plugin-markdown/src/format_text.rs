use dprint_core::*;
use dprint_core::configuration::{resolve_new_line_kind};

use super::configuration::Configuration;
use super::parse_cmark_ast;
use super::parser::parse_node;
use super::parser_types::Context;

/// Formats a file.
///
/// Returns the file text when the file was formatted, `None` when the file had an ignore comment, and
/// an error when it failed to parse.
pub fn format_text(file_text: &str, config: &Configuration) -> Result<Option<String>, String> {
    let source_file = parse_cmark_ast(file_text)?;
    let print_items = parse_node(&source_file.into(), &mut Context::new(file_text));

    Ok(Some(print(print_items, PrintOptions {
        indent_width: config.indent_width,
        max_width: config.line_width,
        use_tabs: config.use_tabs,
        new_line_text: resolve_new_line_kind(file_text, &config.new_line_kind),
    })))
}