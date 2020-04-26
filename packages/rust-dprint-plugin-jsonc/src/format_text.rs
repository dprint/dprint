use dprint_core::{print, PrintOptions};
use dprint_core::configuration::resolve_new_line_kind;
use super::configuration::Configuration;
use super::parser::parse_items;

pub fn format_text(text: &str, config: &Configuration) -> Result<String, String> {
    let print_items = parse_items(text, config)?;

    Ok(print(print_items, PrintOptions {
        indent_width: config.indent_width,
        max_width: config.line_width,
        use_tabs: config.use_tabs,
        new_line_text: resolve_new_line_kind(text, config.new_line_kind),
    }))
}
