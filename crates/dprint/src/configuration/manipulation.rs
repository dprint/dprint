use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::ConfigChangeKind;
use dprint_core::plugins::ConfigChangePathItem;
use jsonc_parser::cst::CstContainerNode;
use jsonc_parser::cst::CstInputValue;
use jsonc_parser::cst::CstLeafNode;
use jsonc_parser::cst::CstNode;
use jsonc_parser::cst::CstObject;
use jsonc_parser::cst::CstRootNode;
use jsonc_parser::json;

use crate::plugins::PluginSourceReference;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::parse_npm_specifier;

#[derive(Debug)]
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

pub fn update_plugin_in_config(file_text: &str, info: &PluginUpdateInfo) -> String {
  let new_url = info.get_full_new_config_url();
  // npm references normalize their display form (the default `/plugin.wasm`
  // path is stripped), so `info.old_reference.to_string()` is often a strict
  // prefix of the original config text. A naive `replace` would leave the
  // `/plugin.wasm[@checksum]` suffix glued to the new version. For npm we
  // always go through the CST path so we never fall back to the buggy
  // prefix-replace; if the entry isn't in this file (e.g. it lives in an
  // `extends`ed config) we return the file unchanged.
  if let PathSource::Npm(_) = &info.old_reference.path_source {
    return replace_npm_plugin_in_config(file_text, &info.old_reference, &new_url);
  }
  file_text.replace(&info.old_reference.to_string(), &new_url)
}

fn replace_npm_plugin_in_config(file_text: &str, old_reference: &PluginSourceReference, new_url: &str) -> String {
  let PathSource::Npm(npm_source) = &old_reference.path_source else {
    return file_text.to_string();
  };
  let Ok(root) = CstRootNode::parse(file_text, &Default::default()) else {
    // unparseable config — don't risk corrupting it via prefix-replace
    return file_text.to_string();
  };
  let Some(plugins) = root.object_value().and_then(|obj| obj.array_value("plugins")) else {
    // no plugins array in this file (e.g. plugins come from an extends);
    // nothing to rewrite here
    return file_text.to_string();
  };
  // collect every matching string lit first, then replace each. The
  // PluginUpdateInfo passed in identifies a single logical entry, but the
  // same string can appear more than once in the array (e.g. via copy/paste);
  // leaving a stale duplicate around would just make the next config update
  // flag it again.
  let mut matches = Vec::new();
  for element in plugins.elements() {
    let CstNode::Leaf(CstLeafNode::StringLit(string_lit)) = element else {
      continue;
    };
    // a string with an invalid JSON escape isn't ours to interpret; skip it
    // and keep scanning the rest of the array.
    let Ok(entry_text) = string_lit.decoded_value() else {
      continue;
    };
    let Ok(parsed) = parse_npm_specifier(&entry_text) else {
      continue;
    };
    if parsed.specifier == npm_source.specifier && parsed.checksum == old_reference.checksum {
      matches.push(string_lit);
    }
  }
  if matches.is_empty() {
    return file_text.to_string();
  }
  for string_lit in matches {
    string_lit.replace_with(json!(new_url));
  }
  root.to_string()
}

pub fn add_to_plugins_array(file_text: &str, url: &str) -> Result<String> {
  let root_node = CstRootNode::parse(file_text, &Default::default()).context("Failed parsing config file.")?;
  let root_obj = root_node.object_value_or_set();
  let plugins = root_obj.array_value_or_set("plugins");
  plugins.ensure_multiline();
  plugins.append(json!(url));
  Ok(root_node.to_string())
}

#[derive(Default)]
pub struct ApplyConfigChangesResult {
  pub new_text: String,
  pub diagnostics: Vec<String>,
}

pub fn apply_config_changes(file_text: &str, plugin_key: &str, changes: &[ConfigChange]) -> ApplyConfigChangesResult {
  let mut diagnostics = Vec::new();
  let root_node = match CstRootNode::parse(file_text, &Default::default()) {
    Ok(root_node) => root_node,
    Err(err) => {
      diagnostics.push(format!("Failed applying change since config file failed to parse: {:#}", err));
      return ApplyConfigChangesResult {
        new_text: file_text.to_string(),
        diagnostics,
      };
    }
  };
  let root_obj = root_node.object_value_or_set();

  for change in changes {
    let Some(plugin_obj) = root_obj.object_value(plugin_key) else {
      return Default::default();
    };
    match &change.kind {
      ConfigChangeKind::Add(value) => {
        if let Err(err) = apply_add(plugin_obj, &change.path, value) {
          diagnostics.push(format!("Failed adding item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      }
      ConfigChangeKind::Set(value) => {
        if let Err(err) = apply_set(plugin_obj, &change.path, value) {
          diagnostics.push(format!("Failed setting item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      }
      ConfigChangeKind::Remove => {
        if let Err(err) = apply_remove(plugin_obj, &change.path) {
          diagnostics.push(format!("Failed removing item at path '{}': {}", display_path(plugin_key, &change.path), err));
        }
      }
    };
  }

  ApplyConfigChangesResult {
    new_text: root_node.to_string(),
    diagnostics,
  }
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

fn apply_add(plugin_obj: CstObject, path: &[ConfigChangePathItem], value: &ConfigKeyValue) -> Result<()> {
  let mut current_node: CstNode = plugin_obj.into();
  for (path_index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        if path_index == path.len() - 1 {
          let maybe_array_prop = current_node.as_object().and_then(|obj| obj.array_value(key));
          match maybe_array_prop {
            Some(array) => {
              array.append(config_value_to_cst_json(value));
              return Ok(());
            }
            None => {
              if let Some(obj) = current_node.as_object() {
                obj.append(key, config_value_to_cst_json(value));
                return Ok(());
              } else {
                bail!("Unsupported. Could not add into {:?} with string key '{}'", current_node.to_string(), key)
              }
            }
          }
        } else {
          let property = current_node
            .as_object()
            .and_then(|obj| obj.get(key))
            .ok_or_else(|| anyhow!("Expected property '{}'.", key))?;
          let value = property.value().ok_or_else(|| anyhow!("Expected value for property '{}'.", key))?;
          current_node = value;
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array."))?;
        if path_index == path.len() - 1 {
          array.insert(array_index, config_value_to_cst_json(value));
          return Ok(());
        } else {
          let mut elements = array.elements();
          if array_index >= elements.len() {
            bail!("Expected array index '{}' to be less than the length of the array.", array_index);
          }
          current_node = elements.remove(array_index);
        }
      }
    }
  }

  bail!("Failed to discover item to add to.")
}

fn apply_set(plugin_obj: CstObject, path: &[ConfigChangePathItem], value: &ConfigKeyValue) -> Result<()> {
  fn replace_node(node: CstNode, value: CstInputValue) -> Result<()> {
    match node {
      CstNode::Container(n) => match n {
        CstContainerNode::Root(_) => unreachable!(),
        CstContainerNode::Array(n) => {
          n.replace_with(value);
        }
        CstContainerNode::Object(n) => {
          n.replace_with(value);
        }
        CstContainerNode::ObjectProp(_) => {
          bail!("Cannot replace an object property.");
        }
      },
      CstNode::Leaf(n) => match n {
        CstLeafNode::BooleanLit(n) => {
          n.replace_with(value);
        }
        CstLeafNode::NullKeyword(n) => {
          n.replace_with(value);
        }
        CstLeafNode::NumberLit(n) => {
          n.replace_with(value);
        }
        CstLeafNode::StringLit(n) => {
          n.replace_with(value);
        }
        CstLeafNode::WordLit(n) => {
          n.replace_with(value);
        }
        CstLeafNode::Token(_) | CstLeafNode::Whitespace(_) | CstLeafNode::Newline(_) | CstLeafNode::Comment(_) => unreachable!(),
      },
    }
    Ok(())
  }

  let mut current_node: CstNode = plugin_obj.into();
  for (path_index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let property = current_node
          .as_object()
          .and_then(|obj| obj.get(key))
          .ok_or_else(|| anyhow!("Expected property '{}'.", key))?;
        let property_value = property.value().ok_or_else(|| anyhow!("Expected value for property '{}'.", key))?;
        if path_index == path.len() - 1 {
          return replace_node(property_value, config_value_to_cst_json(value));
        } else {
          current_node = property_value;
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array."))?;
        let mut elements = array.elements();
        if array_index >= elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", array_index);
        }
        let element = elements.remove(array_index);
        if path_index == path.len() - 1 {
          return replace_node(element, config_value_to_cst_json(value));
        } else {
          current_node = element;
        }
      }
    }
  }

  bail!("Failed to discover item to set.")
}

fn apply_remove(plugin_obj: CstObject, path: &[ConfigChangePathItem]) -> Result<()> {
  let mut current_node: CstNode = plugin_obj.into();
  for (path_index, path_item) in path.iter().enumerate() {
    match path_item {
      ConfigChangePathItem::String(key) => {
        let obj = current_node.as_object().ok_or_else(|| anyhow!("Expected object for property '{}'.", key))?;
        let property = obj.get(key).ok_or_else(|| anyhow!("Expected property '{}'.", key))?;
        if path_index == path.len() - 1 {
          property.remove();
          return Ok(());
        } else {
          current_node = property.value().ok_or_else(|| anyhow!("Failed to find value for property '{}'.", key))?;
        }
      }
      ConfigChangePathItem::Number(array_index) => {
        let array_index = *array_index;
        let array = current_node.as_array().ok_or_else(|| anyhow!("Expected array."))?;
        let mut elements = array.elements();
        if array_index >= elements.len() {
          bail!("Expected array index '{}' to be less than the length of the array.", array_index);
        }
        let element = elements.remove(array_index);
        if path_index == path.len() - 1 {
          element.remove();
          return Ok(());
        } else {
          current_node = element;
        }
      }
    }
  }

  bail!("Failed to discover item to remove.")
}

fn config_value_to_cst_json(value: &ConfigKeyValue) -> CstInputValue {
  match value {
    ConfigKeyValue::Bool(value) => CstInputValue::Bool(*value),
    ConfigKeyValue::Number(value) => CstInputValue::Number(value.to_string()),
    ConfigKeyValue::String(value) => CstInputValue::String(value.clone()),
    ConfigKeyValue::Array(values) => CstInputValue::Array(values.iter().map(config_value_to_cst_json).collect()),
    ConfigKeyValue::Object(values) => CstInputValue::Object(values.iter().map(|(key, value)| (key.clone(), config_value_to_cst_json(value))).collect()),
    ConfigKeyValue::Null => CstInputValue::Null,
  }
}

#[cfg(test)]
mod test {
  use dprint_core::configuration::ConfigKeyMap;
  use pretty_assertions::assert_eq;

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

    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    // some comment
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
    "value",
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

    assert_eq!(
      final_text,
      r#"{
  "plugins": [
    "some_value", // comment
    "value"
  ]
}"#
    );
  }

  #[test]
  fn update_plugin_in_config_npm_rewrites_explicit_default_path_entry() {
    // user wrote `npm:foo@1.0.0/plugin.wasm` (explicit default path). The
    // PluginSourceReference normalizes the path to "plugin.wasm" and
    // display() strips it, so a naive string replace would leave a stale
    // `/plugin.wasm` suffix. The structural update should replace the whole
    // entry cleanly.
    use crate::plugins::PluginSourceReference;
    use crate::utils::NpmSpecifier;
    let npm = |v: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some(v.to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };
    let info = PluginUpdateInfo {
      name: "foo".to_string(),
      old_version: "1.0.0".to_string(),
      old_reference: npm("1.0.0"),
      new_version: "1.1.0".to_string(),
      new_reference: npm("1.1.0"),
    };
    let input = r#"{
  "plugins": [
    "npm:foo@1.0.0/plugin.wasm"
  ]
}"#;
    let expected = r#"{
  "plugins": [
    "npm:foo@1.1.0"
  ]
}"#;
    assert_eq!(update_plugin_in_config(input, &info), expected);
  }

  #[test]
  fn update_plugin_in_config_npm_rewrites_process_plugin_with_checksum() {
    // process plugins always carry a checksum; both the path and the
    // checksum need to be replaced as a single unit.
    use crate::plugins::PluginSourceReference;
    use crate::utils::NpmSpecifier;
    let npm = |v: &str, csum: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some(v.to_string()),
          path: "plugin.json".to_string(),
        },
        None,
      ),
      checksum: Some(csum.to_string()),
    };
    let info = PluginUpdateInfo {
      name: "foo".to_string(),
      old_version: "1.0.0".to_string(),
      old_reference: npm("1.0.0", "oldsum"),
      new_version: "1.1.0".to_string(),
      new_reference: npm("1.1.0", "newsum"),
    };
    let input = r#"{
  "plugins": [
    "npm:foo@1.0.0/plugin.json@oldsum"
  ]
}"#;
    let expected = r#"{
  "plugins": [
    "npm:foo@1.1.0/plugin.json@newsum"
  ]
}"#;
    assert_eq!(update_plugin_in_config(input, &info), expected);
  }

  #[test]
  fn update_plugin_in_config_npm_rewrites_duplicate_entries() {
    // a plugins array with two identical npm references — both should be
    // bumped, not just the first. Otherwise the next dprint config update
    // would flag the stale duplicate again.
    use crate::plugins::PluginSourceReference;
    use crate::utils::NpmSpecifier;
    let npm = |v: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some(v.to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };
    let info = PluginUpdateInfo {
      name: "foo".to_string(),
      old_version: "1.0.0".to_string(),
      old_reference: npm("1.0.0"),
      new_version: "1.1.0".to_string(),
      new_reference: npm("1.1.0"),
    };
    let input = r#"{
  "plugins": [
    "npm:foo@1.0.0",
    "npm:foo@1.0.0"
  ]
}"#;
    let expected = r#"{
  "plugins": [
    "npm:foo@1.1.0",
    "npm:foo@1.1.0"
  ]
}"#;
    assert_eq!(update_plugin_in_config(input, &info), expected);
  }

  #[test]
  fn update_plugin_in_config_npm_returns_file_unchanged_when_no_plugins_array() {
    // e.g. the plugin reference lives in an `extends`ed file, so this file
    // has no plugins array of its own. The previous fallback to file_text
    // .replace would search for the normalized display ("npm:foo@1.0.0")
    // anywhere in the text — including comments or other strings — and
    // potentially corrupt them.
    use crate::plugins::PluginSourceReference;
    use crate::utils::NpmSpecifier;
    let npm = |v: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some(v.to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };
    let info = PluginUpdateInfo {
      name: "foo".to_string(),
      old_version: "1.0.0".to_string(),
      old_reference: npm("1.0.0"),
      new_version: "1.1.0".to_string(),
      new_reference: npm("1.1.0"),
    };
    let input = r#"{
  "extends": "./shared.json",
  "//": "would have been corrupted by a textual replace of npm:foo@1.0.0"
}"#;
    assert_eq!(update_plugin_in_config(input, &info), input);
  }

  #[test]
  fn update_plugin_in_config_npm_leaves_other_entries_alone() {
    // a similarly-prefixed but distinct entry (different path) must not be
    // touched. A naive prefix-replace would have rewritten both.
    use crate::plugins::PluginSourceReference;
    use crate::utils::NpmSpecifier;
    let npm_wasm = |v: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some(v.to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };
    let info = PluginUpdateInfo {
      name: "foo".to_string(),
      old_version: "1.0.0".to_string(),
      old_reference: npm_wasm("1.0.0"),
      new_version: "1.1.0".to_string(),
      new_reference: npm_wasm("1.1.0"),
    };
    let input = r#"{
  "plugins": [
    "npm:foo@1.0.0",
    "npm:foo@1.0.0/plugin.json@somesum"
  ]
}"#;
    let expected = r#"{
  "plugins": [
    "npm:foo@1.1.0",
    "npm:foo@1.0.0/plugin.json@somesum"
  ]
}"#;
    assert_eq!(update_plugin_in_config(input, &info), expected);
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
          "sub": ["test"]
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
        [true, "value"]
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

    // remove and add
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
          path: vec!["add".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(true)),
        },
        ConfigChange {
          path: vec!["prop".to_string().into(), 0.into()],
          kind: ConfigChangeKind::Remove,
        },
        ConfigChange {
          path: vec!["prop".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Bool(false)),
        },
      ],
      r#"{
  "plugin": {
    "prop": [
      2,
      false
    ],
    "add": true
  }
}"#,
      &[],
    );
  }

  #[track_caller]
  fn run_config_change_test(file_text: &str, changes: &[ConfigChange], expected_text: &str, diagnostics: &[&str]) {
    let result = apply_config_changes(file_text, "plugin", changes);
    assert_eq!(result.diagnostics, diagnostics);
    assert_eq!(result.new_text, expected_text);
  }
}
