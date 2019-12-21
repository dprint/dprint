use super::*;
use dprint_core::*;

pub fn format_text(file_path: &str, file_text: &str) -> Result<String, String> {
    let parsed_source_file = parse_to_swc_ast(&file_path, &file_text)?;
    let print_items = parse(parsed_source_file);

    Ok(print(print_items, PrintOptions {
        // todo: configuration
        indent_width: 4,
        max_width: 10,
        is_testing: false,
        use_tabs: false,
        newline_kind: "\n",
    }))
}