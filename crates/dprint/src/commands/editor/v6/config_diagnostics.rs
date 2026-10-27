//! Finds where the problems of a config file are in its text.

use deno_tower_lsp::lsp_types::Range;
use jsonc_parser::ast::Object;
use jsonc_parser::ast::ObjectProp;
use jsonc_parser::ast::ObjectPropName;
use jsonc_parser::ast::Value;
use text_size::TextSize;

use crate::commands::lsp::text::LineIndex;
use crate::configuration::GlobalConfigDiagnostic;
use crate::plugins::PluginConfigDiagnostic;

use super::messages::ConfigDiagnostic;

/// A problem with the configuration of a plugin along with the key of the
/// plugin's section in the config file.
pub struct PluginSectionDiagnostic<'a> {
  pub config_key: &'a str,
  pub diagnostic: &'a PluginConfigDiagnostic,
}

/// Gets the diagnostics of the config file, finding where each one's property
/// is in the text of the config file when it's there.
pub fn get_config_diagnostics(
  config_text: Option<&str>,
  global_diagnostics: &[GlobalConfigDiagnostic],
  plugin_diagnostics: &[PluginSectionDiagnostic],
) -> Vec<ConfigDiagnostic> {
  let text = ConfigText::new(config_text);
  let root = text.root();
  let mut diagnostics = Vec::with_capacity(global_diagnostics.len() + plugin_diagnostics.len());
  for diagnostic in global_diagnostics {
    let diagnostic = match diagnostic {
      GlobalConfigDiagnostic::UnknownProperty(diagnostic) | GlobalConfigDiagnostic::Other(diagnostic) => diagnostic,
    };
    let prop = root.and_then(|root| root.get(&diagnostic.property_name));
    diagnostics.push(ConfigDiagnostic {
      message: diagnostic.message.clone(),
      plugin_name: None,
      property_name: Some(diagnostic.property_name.clone()),
      range: prop.and_then(|prop| text.range(prop_name_range(prop))),
    });
  }
  for item in plugin_diagnostics {
    let section = root.and_then(|root| root.get(item.config_key));
    let property_name = &item.diagnostic.diagnostic.property_name;
    let prop = section.and_then(|section| find_plugin_property(section, property_name));
    diagnostics.push(ConfigDiagnostic {
      message: item.diagnostic.diagnostic.message.clone(),
      plugin_name: Some(item.diagnostic.plugin_name.clone()),
      property_name: Some(property_name.clone()),
      // the plugin's section when the property isn't found (ex. it's in an extended config file)
      range: prop.or(section).and_then(|prop| text.range(prop_name_range(prop))),
    });
  }
  diagnostics
}

/// Gets the diagnostic of a config file that failed to resolve, which is
/// where the text doesn't parse when that's why.
pub fn get_config_error_diagnostic(config_text: Option<&str>, err: &anyhow::Error) -> ConfigDiagnostic {
  let parse_error_range = config_text.and_then(|config_text| {
    let err = parse(config_text).err()?;
    ConfigText::new(Some(config_text)).range(err.range())
  });
  ConfigDiagnostic {
    message: format!("{:#}", err),
    plugin_name: None,
    property_name: None,
    range: parse_error_range,
  }
}

/// Finds the property of a plugin's section, which may be in its overrides.
fn find_plugin_property<'a, 'b>(section: &'b ObjectProp<'a>, property_name: &str) -> Option<&'b ObjectProp<'a>> {
  let Value::Object(section) = &section.value else {
    return None;
  };
  if let Some(prop) = section.get(property_name) {
    return Some(prop);
  }
  let overrides = section.get("overrides")?;
  let find_in = |value: &'b Value<'a>| match value {
    Value::Object(obj) => obj.get(property_name),
    _ => None,
  };
  match &overrides.value {
    Value::Array(array) => array.elements.iter().find_map(find_in),
    value => find_in(value),
  }
}

fn prop_name_range(prop: &ObjectProp) -> jsonc_parser::common::Range {
  match &prop.name {
    ObjectPropName::String(lit) => lit.range,
    ObjectPropName::Word(lit) => lit.range,
  }
}

fn parse(text: &str) -> Result<Option<Value<'_>>, jsonc_parser::errors::ParseError> {
  jsonc_parser::parse_to_ast(text, &Default::default(), &Default::default()).map(|result| result.value)
}

/// The text of a config file, which is `None` when it's not known (ex. a
/// remote config file).
struct ConfigText<'a> {
  text: Option<&'a str>,
  value: Option<Value<'a>>,
  line_index: Option<LineIndex>,
}

impl<'a> ConfigText<'a> {
  fn new(text: Option<&'a str>) -> Self {
    Self {
      text,
      value: text.and_then(|text| parse(text).ok().flatten()),
      line_index: text.map(LineIndex::new),
    }
  }

  fn root(&self) -> Option<&Object<'a>> {
    match &self.value {
      Some(Value::Object(obj)) => Some(obj),
      _ => None,
    }
  }

  /// Converts the byte range to a range of lines and UTF-16 characters.
  fn range(&self, range: jsonc_parser::common::Range) -> Option<Range> {
    let line_index = self.line_index.as_ref()?;
    let len = self.text?.len();
    let position = |offset: usize| line_index.position_utf16_from_utf8_offset(TextSize::from(std::cmp::min(offset, len) as u32));
    Some(Range {
      start: position(range.start),
      end: position(range.end),
    })
  }
}

#[cfg(test)]
mod test {
  use deno_tower_lsp::lsp_types::Position;
  use dprint_core::configuration::ConfigurationDiagnostic;
  use pretty_assertions::assert_eq;

  use super::*;

  fn range(start: (u32, u32), end: (u32, u32)) -> Option<Range> {
    Some(Range {
      start: Position::new(start.0, start.1),
      end: Position::new(end.0, end.1),
    })
  }

  fn diagnostic(property_name: &str, message: &str) -> ConfigurationDiagnostic {
    ConfigurationDiagnostic {
      property_name: property_name.to_string(),
      message: message.to_string(),
    }
  }

  #[test]
  fn should_find_the_properties_of_diagnostics() {
    let text = r#"{
  "unknown": 1,
  "typescript": {
    "lineWidth": "a",
    "overrides": [{ "files": "*.ts", "badOverride": 1 }]
  },
  // the plugin's section is used when the property isn't in it
  "json": {}
}"#;
    let plugin_diagnostics = [
      PluginConfigDiagnostic {
        plugin_name: "dprint-plugin-typescript".to_string(),
        diagnostic: diagnostic("lineWidth", "Expected a number"),
      },
      PluginConfigDiagnostic {
        plugin_name: "dprint-plugin-typescript".to_string(),
        diagnostic: diagnostic("badOverride", "Unknown property in configuration"),
      },
      PluginConfigDiagnostic {
        plugin_name: "dprint-plugin-json".to_string(),
        diagnostic: diagnostic("extended", "Unknown property in configuration"),
      },
      PluginConfigDiagnostic {
        plugin_name: "dprint-plugin-markdown".to_string(),
        diagnostic: diagnostic("other", "Unknown property in configuration"),
      },
    ];
    let config_keys = ["typescript", "typescript", "json", "markdown"];
    let plugin_section_diagnostics = plugin_diagnostics
      .iter()
      .zip(config_keys)
      .map(|(diagnostic, config_key)| PluginSectionDiagnostic { config_key, diagnostic })
      .collect::<Vec<_>>();
    let diagnostics = get_config_diagnostics(
      Some(text),
      &[GlobalConfigDiagnostic::UnknownProperty(diagnostic(
        "unknown",
        "Unknown property in configuration",
      ))],
      &plugin_section_diagnostics,
    );
    assert_eq!(
      diagnostics.into_iter().map(|d| (d.plugin_name, d.property_name, d.range)).collect::<Vec<_>>(),
      vec![
        (None, Some("unknown".to_string()), range((1, 2), (1, 11))),
        (
          Some("dprint-plugin-typescript".to_string()),
          Some("lineWidth".to_string()),
          range((3, 4), (3, 15))
        ),
        (
          Some("dprint-plugin-typescript".to_string()),
          Some("badOverride".to_string()),
          range((4, 37), (4, 50))
        ),
        (Some("dprint-plugin-json".to_string()), Some("extended".to_string()), range((7, 2), (7, 8))),
        (Some("dprint-plugin-markdown".to_string()), Some("other".to_string()), None),
      ]
    );
  }

  #[test]
  fn should_get_where_a_config_file_does_not_parse() {
    let err = anyhow::anyhow!("Failed parsing.");
    let diagnostic = get_config_error_diagnostic(Some("{\n  \"a\": }"), &err);
    assert_eq!(diagnostic.message, "Failed parsing.");
    assert_eq!(diagnostic.range, range((1, 7), (1, 8)));
    // it parses, so the problem is elsewhere
    assert_eq!(get_config_error_diagnostic(Some("{}"), &err).range, None);
    assert_eq!(get_config_error_diagnostic(None, &err).range, None);
  }
}
