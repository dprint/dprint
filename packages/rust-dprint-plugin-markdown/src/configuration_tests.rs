use std::collections::HashMap;
use dprint_core::configuration::{resolve_global_config, NewLineKind};
use super::configuration::*;

#[test]
fn check_all_values_set() {
    let mut config = ConfigurationBuilder::new();
    config.new_line_kind(NewLineKind::CarriageReturnLineFeed)
        .line_width(90)
        .use_tabs(true)
        .indent_width(8);

    let inner_config = config.get_inner_config();
    assert_eq!(inner_config.len(), 4);
    let diagnostics = resolve_config(inner_config, &resolve_global_config(HashMap::new()).config).diagnostics;
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn handle_global_config() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("lineWidth"), String::from("90"));
    global_config.insert(String::from("indentWidth"), String::from("8"));
    global_config.insert(String::from("newLineKind"), String::from("crlf"));
    global_config.insert(String::from("useTabs"), String::from("true"));
    let global_config = resolve_global_config(global_config).config;
    let mut config_builder = ConfigurationBuilder::new();
    let config = config_builder.global_config(global_config).build();
    assert_eq!(config.line_width, 90);
    assert_eq!(config.indent_width, 8);
    assert_eq!(config.new_line_kind == NewLineKind::CarriageReturnLineFeed, true);
    assert_eq!(config.use_tabs, true);
}

#[test]
fn use_markdown_defaults_when_global_not_set() {
    let global_config = resolve_global_config(HashMap::new()).config;
    let mut config_builder = ConfigurationBuilder::new();
    let config = config_builder.global_config(global_config).build();
    assert_eq!(config.line_width, 80); // this is different
    assert_eq!(config.indent_width, 4);
    assert_eq!(config.new_line_kind == NewLineKind::LineFeed, true);
    assert_eq!(config.use_tabs, false);
}
