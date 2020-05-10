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

#[cfg(test)]
mod tests {
    use dprint_core::configuration::*;
    use std::collections::HashMap;
    use super::super::configuration::resolve_config;
    use super::*;

    #[test]
    fn should_error_on_syntax_diagnostic() {
        let global_config = resolve_global_config(HashMap::new()).config;
        let config = resolve_config(HashMap::new(), &global_config).config;
        let message = format_text("{ &*&* }", &config).err().unwrap();
        assert_eq!(
            message,
            concat!(
                "Line 1, column 3: Unexpected token\n",
                "\n",
                "  { &*&* }\n",
                "    ~"
            )
        );
    }
}
