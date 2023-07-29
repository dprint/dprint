use std::collections::HashSet;
use std::ops::Range;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::ConfigChangeKind;
use dprint_core::plugins::ConfigChangePathItem;
use jsonc_parser::ast::Array;
use jsonc_parser::ast::Node;
use jsonc_parser::ast::Object;
use jsonc_parser::common::Ranged;

use crate::plugins::PluginSourceReference;
use crate::utils::PluginKind;
use crate::utils::TextChange;

// This entire module is pretty bad. It would be better to add some manipulation
// capabilities to jsonc-parser.

pub struct PluginUpdateInfo {
  pub name: String,
  pub old_version: String,
  pub old_reference: PluginSourceReference,
  pub new_version: String,
  pub new_reference: PluginSourceReference,
}

impl PluginUpdateInfo {
  pub fn is_wasm(&self) -> bool {
    self.new_reference.plugin_kind() == Some(PluginKind::Wasm)
  }

  pub fn get_full_new_config_url(&self) -> String {
    // only add the checksum if not wasm or previously had a checksum
    let should_add_checksum = !self.is_wasm() || self.old_reference.checksum.is_some();
    if should_add_checksum {
      self.new_reference.to_full_string()
    } else {
      self.new_reference.without_checksum().to_string()
    }
  }
}

pub fn update_plugin_in_config(file_text: &str, info: PluginUpdateInfo) -> String {
  let new_url = info.get_full_new_config_url();
  file_text.replace(&info.old_reference.to_string(), &new_url)
}

pub fn add_to_plugins_array(file_text: &str, url: &str) -> Result<String> {
  let root_obj = JsonRootObject::parse(file_text)?.root;
  let plugins = get_plugins_array(&root_obj)?;
  let indentation_text = get_indentation_text(file_text, &root_obj);
  let newline_char = get_newline_char(file_text);
  // don't worry about comments or anything... too much to deal with
  let start_pos = if let Some(last_element) = plugins.elements.last() {
    last_element.end()
  } else {
    plugins.start() + 1
  };
  let end_pos = plugins.end() - 1;
  let mut final_text = String::new();
  final_text.push_str(&file_text[..start_pos]);
  if !plugins.elements.is_empty() {
    final_text.push(',');
  }
  final_text.push_str(&newline_char);
  final_text.push_str(&indentation_text);
  final_text.push_str(&indentation_text);
  final_text.push_str(&format!("\"{}\"", url));
  final_text.push_str(&newline_char);
  final_text.push_str(&indentation_text);
  final_text.push_str(&file_text[end_pos..]);
  Ok(final_text)
}

pub struct JsonRootObject<'a> {
  root: Object<'a>,
}

impl<'a> JsonRootObject<'a> {
  pub fn parse(file_text: &'a str) -> Result<Self> {
    let json_file =
      jsonc_parser::parse_to_ast(file_text, &Default::default(), &Default::default()).with_context(|| "Error parsing config file.".to_string())?;
    match json_file.value {
      Some(jsonc_parser::ast::Value::Object(obj)) => Ok(Self { root: obj }),
      _ => bail!("Please ensure your config file has an object in it to use this feature."),
    }
  }
}

#[derive(Default)]
pub struct ApplyConfigChangesResult {
  pub text_changes: Vec<TextChange>,
  pub diagnostics: Vec<String>,
}

impl ApplyConfigChangesResult {
  pub fn extend(&mut self, other: ApplyConfigChangesResult) {
    self.text_changes.extend(other.text_changes);
    self.diagnostics.extend(other.diagnostics);
  }
}

#[derive(Clone)]
struct IndentText<'a> {
  text: &'a str,
  level: usize,
}

impl<'a> IndentText<'a> {
  pub fn inc(&self) -> Self {
    Self {
      text: self.text,
      level: self.level + 1,
    }
  }

  pub fn render(&self) -> String {
    self.text.repeat(self.level)
  }
}

pub fn apply_config_changes(file_text: &str, root_obj: &JsonRootObject, plugin_key: &str, changes: &[ConfigChange]) -> ApplyConfigChangesResult {
  let mut diagnostics = Vec::new();
  let Some(plugin_obj) = root_obj.root.get_object(plugin_key) else {
    return Default::default();
  };
  let mut text_changes = Vec::new();
  let indent_text = get_indentation_text(file_text, &root_obj.root);
  let indent_text = IndentText { text: &indent_text, level: 1 };
  let mut add_comma_paths = HashSet::new();

  for change in changes {
    match &change.kind {
      ConfigChangeKind::Add(value) => match apply_add(indent_text.clone(), plugin_obj, &change.path, value, &mut add_comma_paths) {
        Ok(text_change) => {
          text_changes.push(text_change);
        }
        Err(err) => {
          diagnostics.push(format!("Failed adding item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      },
      ConfigChangeKind::Set(value) => match apply_set(plugin_obj, &change.path, value, indent_text.clone()) {
        Ok(text_change) => {
          text_changes.push(text_change);
        }
        Err(err) => {
          diagnostics.push(format!("Failed setting item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      },
      ConfigChangeKind::Remove => match apply_remove(file_text, plugin_obj, &change.path) {
        Ok(range) => {
          text_changes.push(TextChange {
            range,
            new_text: String::new(),
          });
        }
        Err(err) => {
          diagnostics.push(format!("Failed removing item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      },
    }
  }

  ApplyConfigChangesResult { text_changes, diagnostics }
}

fn display_path(plugin_key: &str, path: &[ConfigChangePathItem]) -> String {
  let mut text = plugin_key.to_string();
  for path in path {
    match path {
      ConfigChangePathItem::String(key) => {
        text.push('.');
        text.push_str(key);
      }
      ConfigChangePathItem::Number(index) => {
        text.push('[');
        text.push_str(&index.to_string());
        text.push(']');
      }
    }
  }
  text
}

fn apply_add(
  mut indent_text: IndentText,
  plugin_obj: &Object,
  path: &[ConfigChangePathItem],
  value: &ConfigKeyValue,
  comma_paths: &mut HashSet<String>,
) -> Result<TextChange> {
  let mut current_node = Node::Object(plugin_obj);
  for (path_index, path_item) in path.iter().enumerate() {
    indent_text = indent_text.inc();
    match path_item {
      ConfigChangePathItem::String(key) => {
        if path_index == path.len() - 1 {
          let maybe_array_prop = current_node.as_object().and_then(|obj| obj.get_array(key));
          match maybe_array_prop {
            Some(array) => {
              let indent_text = indent_text.inc();
              match array.elements.last() {
                Some(last_property) => {
                  let prop_end = last_property.range().end();
                  return Ok(TextChange {
                    range: prop_end..prop_end,
                    new_text: format!(",\n{}{}", indent_text.render(), config_value_to_json_text(value, &indent_text)),
                  });
                }
                None => {
                  let leading_comma = !comma_paths.insert(display_path("", path));
                  let after_bracket_pos = array.start() + 1;
                  return Ok(TextChange {
                    range: after_bracket_pos..after_bracket_pos,
                    new_text: format!(
                      "{}\n{}{}",
                      if leading_comma { "," } else { "" },
                      indent_text.render(),
                      config_value_to_json_text(value, &indent_text)
                    ),
                  });
                }
              }
            }
            None => match current_node {
              Node::Object(obj) => match obj.properties.last() {
                Some(last_property) => {
                  let prop_end = last_property.range().end();
                  return Ok(TextChange {
                    range: prop_end..prop_end,
                    new_text: format!(",\n{}\"{}\": {}", indent_text.render(), key, config_value_to_json_text(value, &indent_text)),
                  });
                }
                None => {
                  let leading_comma = !comma_paths.insert(display_path("", &path[0..path.len() - 1]));
                  let after_brace_pos = obj.start() + 1;
                  return Ok(TextChange {
                    range: after_brace_pos..after_brace_pos,
                    new_text: format!(
                      "{}\n{}\"{}\": {}",
                      if leading_comma { "," } else { "" },
                      indent_text.render(),
                      key,
                      config_value_to_json_text(value, &indent_text)
                    ),
                  });
                }
              },
              _ => bail!("Unsupported. Could not add into {:?} with string key '{}'", current_node.kind(), key),
            },
          }
        } else {
          let property = current_node
            .as_object()
            .and_then(|obj| obj.get(key))
            .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
          current_node = (&property.value).into();
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if array_index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", array_index);
        }
        let element = array.elements.get(array_index).unwrap();
        if path_index == path.len() - 1 {
          return Ok(TextChange {
            range: element.end()..element.end(),
            new_text: format!(",\n  {}", config_value_to_json_text(value, &indent_text)),
          });
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to add to.")
}

fn apply_set(plugin_obj: &Object<'_>, path: &[ConfigChangePathItem], value: &ConfigKeyValue, mut indent_text: IndentText) -> Result<TextChange> {
  let mut current_node = Node::Object(plugin_obj);
  for (path_index, path_item) in path.iter().enumerate() {
    indent_text = indent_text.inc();
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
        if path_index == path.len() - 1 {
          return Ok(TextChange {
            range: property.value.start()..property.value.end(),
            new_text: config_value_to_json_text(value, &indent_text),
          });
        } else {
          current_node = (&property.value).into();
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if array_index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", array_index);
        }
        let element = array.elements.get(array_index).unwrap();
        if path_index == path.len() - 1 {
          return Ok(TextChange {
            range: element.start()..element.end(),
            new_text: config_value_to_json_text(value, &indent_text),
          });
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to set.")
}

fn apply_remove(file_text: &str, plugin_obj: &jsonc_parser::ast::Object, path: &[ConfigChangePathItem]) -> Result<Range<usize>> {
  fn get_end_pos(file_text: &str, node: Node) -> usize {
    let end_pos = node.end();
    // very simple, but probably good enough
    let end_pos = if file_text[end_pos..].starts_with(',') { end_pos + 1 } else { end_pos };
    let end_text = &file_text[end_pos..];
    let trimmed_start_end_text = end_text.trim_start();
    let whitespace_width = end_text.len() - trimmed_start_end_text.len();
    end_pos + whitespace_width
  }

  let mut current_node = Node::Object(plugin_obj);
  for (path_index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
        if path_index == path.len() - 1 {
          return Ok(property.start()..get_end_pos(file_text, property.into()));
        } else {
          current_node = (&property.value).into();
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if array_index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", array_index);
        }
        let element = array.elements.get(array_index).unwrap();
        if path_index == path.len() - 1 {
          return Ok(element.start()..get_end_pos(file_text, element.into()));
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to remove.")
}

fn config_value_to_json_text(value: &ConfigKeyValue, indent_text: &IndentText) -> String {
  match value {
    ConfigKeyValue::Bool(value) => value.to_string(),
    ConfigKeyValue::Number(value) => value.to_string(),
    ConfigKeyValue::String(value) => format!("\"{}\"", value.replace('\"', "\\\"")),
    ConfigKeyValue::Array(values) => {
      let mut text = String::new();
      text.push('[');
      for (i, value) in values.iter().enumerate() {
        if i == 0 {
          text.push('\n');
        } else {
          text.push_str(",\n");
        }
        let indent_text = indent_text.inc();
        text.push_str(&indent_text.render());
        text.push_str(&config_value_to_json_text(value, &indent_text));
      }
      text.push_str(&format!("\n{}]", indent_text.render()));
      text
    }
    ConfigKeyValue::Object(values) => {
      let mut text = String::new();
      text.push('{');
      for (i, (key, value)) in values.iter().enumerate() {
        if i == 0 {
          text.push('\n');
        } else {
          text.push_str(",\n");
        }
        let indent_text = indent_text.inc();
        text.push_str(&indent_text.render());
        text.push_str(&format!("\"{}\": {}", key, config_value_to_json_text(value, &indent_text)));
      }
      text.push_str(&format!("\n{}}}", indent_text.render()));
      text
    }
    ConfigKeyValue::Null => "null".to_string(),
  }
}

fn get_plugins_array<'a>(root_obj: &'a Object<'a>) -> Result<&'a Array<'a>> {
  root_obj
    .get_array("plugins")
    .ok_or_else(|| anyhow!("Please ensure your config file has an object with a plugins array in it to use this feature."))
}

fn get_newline_char(file_text: &str) -> String {
  if file_text.contains("\r\n") {
    "\r\n".to_string()
  } else {
    "\n".to_string()
  }
}

fn get_indentation_text(file_text: &str, root_obj: &Object) -> String {
  root_obj
    .properties
    .first()
    .map(|first_property| {
      let after_brace_position = root_obj.start() + 1;
      let first_property_start = first_property.start();
      let text = file_text[after_brace_position..first_property_start].replace('\r', "");
      let last_line = text.split('\n').last().unwrap();
      last_line.chars().take_while(|c| c.is_whitespace()).collect::<String>()
    })
    .unwrap_or_else(|| "  ".to_string())
}

#[cfg(test)]
mod test {
  use dprint_core::configuration::ConfigKeyMap;
  use pretty_assertions::assert_eq;

  use crate::utils::apply_text_changes;

  use super::*;

  #[test]
  pub fn add_plugins_array_empty() {
    let final_text = add_to_plugins_array(
      r#"{
  "plugins": []
}"#,
      "value",
    )
    .unwrap();

    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "value"
  ]
}"#
    );
  }

  #[test]
  pub fn add_plugins_array_empty_comment() {
    let final_text = add_to_plugins_array(
      r#"{
  "plugins": [
    // some comment
  ]
}"#,
      "value",
    )
    .unwrap();

    // don't bother... just remove it
    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "value"
  ]
}"#
    );
  }

  #[test]
  pub fn add_plugins_not_empty() {
    let final_text = add_to_plugins_array(
      r#"{
  "plugins": [
    "some_value"
  ]
}"#,
      "value",
    )
    .unwrap();

    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "some_value",
    "value"
  ]
}"#
    );
  }

  #[test]
  pub fn add_plugins_trailing_comma() {
    let final_text = add_to_plugins_array(
      r#"{
  "plugins": [
    "some_value",
  ]
}"#,
      "value",
    )
    .unwrap();

    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "some_value",
    "value"
  ]
}"#
    );
  }

  #[test]
  pub fn add_plugins_trailing_comment() {
    let final_text = add_to_plugins_array(
      r#"{
  "plugins": [
    "some_value" // comment
  ]
}"#,
      "value",
    )
    .unwrap();

    // just remove it... too much work
    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "some_value",
    "value"
  ]
}"#
    );
  }

  #[test]
  fn test_add_into_object() {
    // adding when there's a child element
    run_config_change_test(
      r#"{
  "plugin": {
    "other": 5
  }
}"#,
      &[ConfigChange {
        path: vec!["test".to_string().into()],
        kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(true)),
      }],
      r#"{
  "plugin": {
    "other": 5,
    "test": true
  }
}"#,
      &[],
    );
    // adding when there's a child element with a trailing comma
    run_config_change_test(
      r#"{
  "plugin": {
    "other": 5,
  }
}"#,
      &[ConfigChange {
        path: vec!["test".to_string().into()],
        kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(true)),
      }],
      r#"{
  "plugin": {
    "other": 5,
    "test": true,
  }
}"#,
      &[],
    );
    // adding when no child element exists
    run_config_change_test(
      r#"{
  "plugin": {
  }
}"#,
      &[
        ConfigChange {
          path: vec!["test".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(true)),
        },
        ConfigChange {
          path: vec!["other".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Object(ConfigKeyMap::from([("test".to_string(), ConfigKeyValue::Bool(true))]))),
        },
      ],
      r#"{
  "plugin": {
    "test": true,
    "other": {
      "test": true
    }
  }
}"#,
      &[],
    );
  }

  #[test]
  fn test_add_into_array() {
    // adding when there's a child element
    run_config_change_test(
      r#"{
  "plugin": {
    "other": [
      "test"
    ]
  }
}"#,
      &[ConfigChange {
        path: vec!["other".to_string().into()],
        kind: ConfigChangeKind::Add(ConfigKeyValue::String("other".to_string())),
      }],
      r#"{
  "plugin": {
    "other": [
      "test",
      "other"
    ]
  }
}"#,
      &[],
    );
    // adding when there's a child element with a trailing comma
    run_config_change_test(
      r#"{
  "plugin": {
    "other": [
      "test",
    ]
  }
}"#,
      &[ConfigChange {
        path: vec!["other".to_string().into()],
        kind: ConfigChangeKind::Add(ConfigKeyValue::Number(5)),
      }],
      r#"{
  "plugin": {
    "other": [
      "test",
      5,
    ]
  }
}"#,
      &[],
    );
    // adding when no child element exists
    run_config_change_test(
      r#"{
  "plugin": {
    "other": [
    ],
    "array": [
      {
        "prop": {
        }
      },
      true,
    ]
  }
}"#,
      &[
        ConfigChange {
          path: vec!["other".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(true)),
        },
        ConfigChange {
          path: vec!["other".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Object(ConfigKeyMap::from([("test".to_string(), ConfigKeyValue::Bool(true))]))),
        },
        ConfigChange {
          path: vec!["array".to_string().into(), 0.into(), "prop".to_string().into(), "sub".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Array(vec!["test".to_string().into()])),
        },
      ],
      r#"{
  "plugin": {
    "other": [
      true,
      {
        "test": true
      }
    ],
    "array": [
      {
        "prop": {
          "sub": [
            "test"
          ]
        }
      },
      true,
    ]
  }
}"#,
      &[],
    );
  }

  #[test]
  fn test_set_values() {
    run_config_change_test(
      r#"{
  "plugin": {
    "other": 5
  }
}"#,
      &[ConfigChange {
        path: vec!["other".to_string().into()],
        kind: ConfigChangeKind::Set(ConfigKeyValue::Bool(true)),
      }],
      r#"{
  "plugin": {
    "other": true
  }
}"#,
      &[],
    );

    run_config_change_test(
      r#"{
  "plugin": {
    "other": [
      "value",
      5,
      2
    ],
    "next": {
      "asdf": [
        true,
        {
          "asdf": 5,
        }
      ]
    }
  }
}"#,
      &[
        ConfigChange {
          path: vec!["other".to_string().into()],
          kind: ConfigChangeKind::Set(ConfigKeyValue::Object(ConfigKeyMap::from([("test".to_string(), ConfigKeyValue::Bool(true))]))),
        },
        ConfigChange {
          path: vec!["next".to_string().into(), "asdf".to_string().into(), 1.into()],
          kind: ConfigChangeKind::Set(ConfigKeyValue::Array(vec![
            ConfigKeyValue::Bool(true),
            ConfigKeyValue::String("value".to_string()),
          ])),
        },
      ],
      r#"{
  "plugin": {
    "other": {
      "test": true
    },
    "next": {
      "asdf": [
        true,
        [
          true,
          "value"
        ]
      ]
    }
  }
}"#,
      &[],
    );
  }

  #[test]
  fn test_remove_values() {
    run_config_change_test(
      r#"{
  "plugin": {
    "other": 5,
    "prop": [
      1,
      2
    ]
  }
}"#,
      &[
        ConfigChange {
          path: vec!["other".to_string().into()],
          kind: ConfigChangeKind::Remove,
        },
        ConfigChange {
          path: vec!["prop".to_string().into(), 0.into()],
          kind: ConfigChangeKind::Remove,
        },
      ],
      r#"{
  "plugin": {
    "prop": [
      2
    ]
  }
}"#,
      &[],
    );
  }

  #[track_caller]
  fn run_config_change_test(file_text: &str, changes: &[ConfigChange], expected_text: &str, diagnostics: &[&str]) {
    let root_obj = JsonRootObject::parse(file_text).unwrap();
    let result = apply_config_changes(file_text, &root_obj, "plugin", changes);
    assert_eq!(result.diagnostics, diagnostics);
    let actual_text = apply_text_changes(file_text, result.text_changes).unwrap();
    assert_eq!(actual_text, expected_text);
  }
}
