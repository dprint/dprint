use dprint_core::*;
use dprint_core::configuration::{resolve_new_line_kind};

use super::configuration::Configuration;
use super::parsing::{parse_cmark_ast, parse_node, Context};

/// Formats a file.
///
/// Returns the file text or an error when it failed to parse.
pub fn format_text(file_text: &str, config: &Configuration) -> Result<String, String> {
    let source_file = match parse_cmark_ast(file_text) {
        Ok(source_file) => source_file,
        Err(error) => {
            return Err(dprint_core::utils::string_utils::format_diagnostic(
                Some((error.range.start, error.range.end)),
                &error.message,
                file_text
            ));
        }
    };
    let print_items = parse_node(&source_file.into(), &mut Context::new(file_text, config));

    Ok(print(print_items, PrintOptions {
        indent_width: config.indent_width,
        max_width: config.line_width,
        use_tabs: config.use_tabs,
        new_line_text: resolve_new_line_kind(file_text, config.new_line_kind),
    }))
}
