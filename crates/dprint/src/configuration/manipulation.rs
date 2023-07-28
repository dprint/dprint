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
  let root_obj = JsonRootObject::parse(file_text)?.0;
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

pub struct JsonRootObject<'a>(Object<'a>);

impl<'a> JsonRootObject<'a> {
  pub fn parse(file_text: &'a str) -> Result<Self> {
    let json_file =
      jsonc_parser::parse_to_ast(file_text, &Default::default(), &Default::default()).with_context(|| "Error parsing config file.".to_string())?;
    match json_file.value {
      Some(jsonc_parser::ast::Value::Object(obj)) => Ok(Self(obj)),
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

pub fn apply_config_changes(root_obj: &JsonRootObject, plugin_key: &str, changes: &[ConfigChange]) -> ApplyConfigChangesResult {
  let mut diagnostics = Vec::new();
  let Some(plugin_obj) = root_obj.0.get_object(plugin_key) else {
    return Default::default();
  };
  let mut text_changes = Vec::new();

  for change in changes {
    match &change.kind {
      ConfigChangeKind::Add(value) => match apply_add(plugin_obj, &change.path, value) {
        Ok(text_change) => {
          text_changes.push(text_change);
        }
        Err(err) => {
          diagnostics.push(format!("Failed adding item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      },
      ConfigChangeKind::Set(value) => match apply_set(plugin_obj, &change.path, value) {
        Ok(text_change) => {
          text_changes.push(text_change);
        }
        Err(err) => {
          diagnostics.push(format!("Failed setting item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      },
      ConfigChangeKind::Remove => match apply_remove(plugin_obj, &change.path) {
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

fn apply_add(plugin_obj: &Object, path: &[ConfigChangePathItem], value: &ConfigKeyValue) -> Result<TextChange> {
  let mut current_node = Node::Object(plugin_obj);
  for (index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
        if index == path.len() - 1 {
          return Ok(TextChange {
            range: property.end()..property.end(),
            new_text: format!(",\n  \"{}\": {}", key, config_value_to_json_text(value)),
          });
        } else {
          current_node = Node::ObjectProp(property);
        }
      }
      ConfigChangePathItem::Number(index) => {
        let index = *index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", index);
        }
        let element = array.elements.get(index).unwrap();
        if index == path.len() - 1 {
          return Ok(TextChange {
            range: element.end()..element.end(),
            new_text: format!(",\n  {}", config_value_to_json_text(value)),
          });
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to add to.")
}

fn apply_set(plugin_obj: &Object<'_>, path: &[ConfigChangePathItem], value: &ConfigKeyValue) -> Result<TextChange> {
  let mut current_node = Node::Object(plugin_obj);
  for (index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
        if index == path.len() - 1 {
          return Ok(TextChange {
            range: property.value.start()..property.value.end(),
            new_text: config_value_to_json_text(value),
          });
        } else {
          current_node = Node::ObjectProp(property);
        }
      }
      ConfigChangePathItem::Number(index) => {
        let index = *index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", index);
        }
        let element = array.elements.get(index).unwrap();
        if index == path.len() - 1 {
          return Ok(TextChange {
            range: element.start()..element.end(),
            new_text: config_value_to_json_text(value),
          });
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to set.")
}

fn config_value_to_json_text(value: &ConfigKeyValue) -> String {
  match value {
    ConfigKeyValue::Bool(value) => value.to_string(),
    ConfigKeyValue::Number(value) => value.to_string(),
    ConfigKeyValue::String(value) => format!("\"{}\"", value.replace('\"', "\\\"")),
    ConfigKeyValue::Array(values) => {
      let mut text = String::new();
      text.push('[');
      for (i, value) in values.iter().enumerate() {
        if i != 0 {
          text.push_str(", ");
        }
        text.push_str(&config_value_to_json_text(value));
      }
      text.push(']');
      text
    }
    ConfigKeyValue::Object(values) => {
      let mut text = String::new();
      text.push('{');
      for (i, (key, value)) in values.iter().enumerate() {
        if i != 0 {
          text.push_str(", ");
        }
        text.push_str(&format!("\"{}\": {}", key, config_value_to_json_text(value)));
      }
      text.push('}');
      text
    }
    ConfigKeyValue::Null => "null".to_string(),
  }
}

fn apply_remove(plugin_obj: &jsonc_parser::ast::Object, path: &[ConfigChangePathItem]) -> Result<Range<usize>> {
  let mut current_node = Node::Object(plugin_obj);
  for (index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}' in path.", key))?;
        if index == path.len() - 1 {
          return Ok(property.start()..property.end());
        } else {
          current_node = Node::ObjectProp(property);
        }
      }
      ConfigChangePathItem::Number(index) => {
        let index = *index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array in path."))?;
        if index >= array.elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", index);
        }
        let element = array.elements.get(index).unwrap();
        if index == path.len() - 1 {
          return Ok(element.start()..element.end());
        } else {
          current_node = element.into();
        }
      }
    }
  }

  bail!("Failed to discover item to remove.")
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
}
