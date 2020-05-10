use std::collections::HashMap;
use super::configuration::*;

#[test]
fn get_default_config_when_empty() {
    let config_result = resolve_global_config(HashMap::new());
    let config = config_result.config;
    assert_eq!(config_result.diagnostics.len(), 0);
    assert_eq!(config.line_width, None);
    assert_eq!(config.indent_width, None);
    assert_eq!(config.new_line_kind.is_none(), true);
    assert_eq!(config.use_tabs, None);
}

#[test]
fn get_values_when_filled() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("lineWidth"), String::from("80"));
    global_config.insert(String::from("indentWidth"), String::from("8"));
    global_config.insert(String::from("newLineKind"), String::from("crlf"));
    global_config.insert(String::from("useTabs"), String::from("true"));
    let config_result = resolve_global_config(global_config);
    let config = config_result.config;
    assert_eq!(config_result.diagnostics.len(), 0);
    assert_eq!(config.line_width, Some(80));
    assert_eq!(config.indent_width, Some(8));
    assert_eq!(config.new_line_kind == Some(NewLineKind::CarriageReturnLineFeed), true);
    assert_eq!(config.use_tabs, Some(true));
}

#[test]
fn get_diagnostic_for_invalid_enum_config() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("newLineKind"), String::from("something"));
    let diagnostics = resolve_global_config(global_config).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Error parsing configuration value for 'newLineKind'. Message: Found invalid value 'something'.");
    assert_eq!(diagnostics[0].property_name, "newLineKind");
}

#[test]
fn get_diagnostic_for_invalid_primitive() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("useTabs"), String::from("something"));
    let diagnostics = resolve_global_config(global_config).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Error parsing configuration value for 'useTabs'. Message: provided string was not `true` or `false`");
    assert_eq!(diagnostics[0].property_name, "useTabs");
}

#[test]
fn get_diagnostic_for_excess_property() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("something"), String::from("value"));
    let diagnostics = resolve_global_config(global_config).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Unknown property in configuration: something");
    assert_eq!(diagnostics[0].property_name, "something");
}
