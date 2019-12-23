use super::*;
use dprint_core::*;

pub fn format_text(file_path: &str, file_text: &str, config: &TypeScriptConfiguration) -> Result<String, String> {
    let parsed_source_file = parse_to_swc_ast(&file_path, &file_text)?;
    let print_items = parse(parsed_source_file, config.clone());

    Ok(print(print_items, PrintOptions {
        // todo: configuration
        indent_width: 4,
        max_width: config.line_width,
        is_testing: false,
        use_tabs: false,
        newline_kind: "\n",
    }))
}