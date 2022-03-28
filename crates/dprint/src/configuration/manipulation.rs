use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use jsonc_parser::ast::Array;
use jsonc_parser::ast::Object;
use jsonc_parser::common::Ranged;

use crate::plugins::PluginSourceReference;

pub struct PluginUpdateInfo {
  pub name: String,
  pub old_version: String,
  pub old_reference: PluginSourceReference,
  pub new_version: String,
  pub new_reference: PluginSourceReference,
}

impl PluginUpdateInfo {
  pub fn is_wasm(&self) -> bool {
    self.new_reference.is_wasm_plugin()
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
  let obj = parse_file(file_text)?;
  let plugins = get_plugins_array(&obj)?;
  let indentation_text = get_indentation_text(file_text, &obj);
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

fn parse_file(file_text: &str) -> Result<Object<'_>> {
  let json_file = jsonc_parser::parse_to_ast(file_text, &Default::default()).with_context(|| "Error parsing config file.".to_string())?;
  match json_file.value {
    Some(jsonc_parser::ast::Value::Object(obj)) => Ok(obj),
    _ => bail!("Please ensure your config file has an object in it to use this feature."),
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
