use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyValue;
use jsonc_parser::JsonArray;
use jsonc_parser::JsonObject;
use jsonc_parser::JsonValue;
use std::collections::HashMap;

use super::ConfigMap;
use super::ConfigMapValue;
use super::RawPluginConfig;

pub fn deserialize_config(config_file_text: &str) -> Result<ConfigMap> {
  let value = jsonc_parser::parse_to_value(config_file_text)?;

  let root_object_node = match value {
    Some(JsonValue::Object(obj)) => obj,
    _ => bail!("Expected a root object in the json"),
  };

  let mut properties = HashMap::new();

  for (key, value) in root_object_node.into_iter() {
    let property_name = key;
    let property_value = match value {
      JsonValue::Object(obj) => ConfigMapValue::PluginConfig(json_obj_to_raw_plugin_config(&property_name, obj)?),
      JsonValue::Array(arr) => ConfigMapValue::Vec(json_array_to_vec(&property_name, arr)?),
      JsonValue::Boolean(value) => ConfigMapValue::from_bool(value),
      JsonValue::String(value) => ConfigMapValue::KeyValue(ConfigKeyValue::String(value.into_owned())),
      JsonValue::Number(value) => ConfigMapValue::from_i32(match value.parse::<i32>() {
        Ok(value) => value,
        Err(err) => {
          bail!(
            "Expected property '{}' with value '{}' to be convertable to a signed integer. {}",
            property_name,
            value,
            err.to_string()
          )
        }
      }),
      _ => bail!("Expected an object, boolean, string, or number in root object property '{}'", property_name),
    };
    properties.insert(property_name, property_value);
  }

  Ok(properties)
}

fn json_obj_to_raw_plugin_config(parent_prop_name: &str, obj: JsonObject) -> Result<RawPluginConfig> {
  let mut properties = HashMap::new();
  let mut locked = false;
  let mut associations = None;

  for (key, value) in obj.into_iter() {
    let property_name = key;
    if property_name == "locked" {
      match value {
        JsonValue::Boolean(value) => {
          locked = value;
          continue;
        }
        _ => bail!("The 'locked' property in a plugin configuration must be a boolean."),
      }
    }

    if property_name == "associations" {
      match value {
        JsonValue::Array(value) => {
          let mut items = Vec::new();
          for value in value.into_iter() {
            match value {
              JsonValue::String(value) => items.push(value.into_owned()),
              _ => bail!("The 'associations' array in a plugin configuration must contain only strings."),
            }
          }
          associations = Some(items);
          continue;
        }
        JsonValue::String(value) => {
          associations = Some(vec![value.into_owned()]);
          continue;
        }
        _ => bail!("The 'associations' property in a plugin configuration must be a string or an array of strings."),
      }
    }

    let property_value = match value_to_plugin_config_key_value(value) {
      Ok(result) => result,
      Err(err) => bail!("{} in object property '{} -> {}'", err, parent_prop_name, property_name),
    };
    properties.insert(property_name, property_value);
  }

  Ok(RawPluginConfig {
    locked,
    associations,
    properties,
  })
}

fn json_array_to_vec(parent_prop_name: &str, array: JsonArray) -> Result<Vec<String>> {
  let mut elements = Vec::new();

  for element in array.into_iter() {
    let value = match value_to_string(element) {
      Ok(result) => result,
      Err(err) => bail!("{} in array '{}'", err, parent_prop_name),
    };
    elements.push(value);
  }

  Ok(elements)
}

fn value_to_string(value: JsonValue) -> Result<String> {
  match value {
    JsonValue::String(value) => Ok(value.into_owned()),
    _ => bail!("Expected a string"),
  }
}

fn value_to_plugin_config_key_value(value: JsonValue) -> Result<ConfigKeyValue> {
  Ok(match value {
    JsonValue::Boolean(value) => ConfigKeyValue::Bool(value),
    JsonValue::String(value) => ConfigKeyValue::String(value.into_owned()),
    JsonValue::Number(value) => ConfigKeyValue::Number(value.parse::<i32>()?),
    _ => bail!("Expected a boolean, string, or number"),
  })
}

#[cfg(test)]
mod tests {
  use crate::configuration::ConfigMap;
  use crate::configuration::ConfigMapValue;
  use crate::configuration::RawPluginConfig;

  use super::deserialize_config;
  use dprint_core::configuration::ConfigKeyValue;
  use std::collections::HashMap;

  #[test]
  fn should_error_when_there_is_a_parser_error() {
    assert_error("{prop}", "Unexpected token on line 1 column 2.");
  }

  #[test]
  fn should_error_when_no_object_in_root() {
    assert_error("[]", "Expected a root object in the json");
  }

  #[test]
  fn should_error_when_the_root_property_has_an_unexpected_value_type() {
    assert_error(
      "{'prop': null}",
      "Expected an object, boolean, string, or number in root object property 'prop'",
    );
  }

  #[test]
  fn should_error_when_the_sub_object_has_object() {
    assert_error(
      "{'prop': { 'test': {}}}",
      "Expected a boolean, string, or number in object property 'prop -> test'",
    );
  }

  #[test]
  fn should_deserialize_empty_object() {
    assert_deserializes("{}", HashMap::new());
  }

  #[test]
  fn should_deserialize_full_object() {
    let mut expected_props = HashMap::new();
    expected_props.insert(String::from("includes"), ConfigMapValue::Vec(Vec::new()));
    expected_props.insert(
      String::from("typescript"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: false,
        associations: None,
        properties: HashMap::from([
          (String::from("lineWidth"), ConfigKeyValue::from_i32(40)),
          (String::from("preferSingleLine"), ConfigKeyValue::from_bool(true)),
          (String::from("other"), ConfigKeyValue::from_str("test")),
        ]),
      }),
    );
    assert_deserializes(
      "{'includes': [], 'typescript': { 'lineWidth': 40, 'preferSingleLine': true, 'other': 'test' }}",
      expected_props,
    );
  }

  #[test]
  fn should_deserialize_cli_specific_plugin_config() {
    let expected_props = HashMap::from([
      (
        "typescript".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: true,
          associations: Some(vec!["test".to_string()]),
          properties: HashMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(40))]),
        }),
      ),
      (
        "other".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: Some(vec!["other".to_string(), "test".to_string()]),
          properties: HashMap::new(),
        }),
      ),
    ]);
    assert_deserializes(
      "{'typescript': { 'lineWidth': 40, locked: true, associations: 'test' }, 'other': { 'locked': false, 'associations': ['other', 'test'] }}",
      expected_props,
    );
  }

  #[test]
  fn error_invalid_cli_specific_properties() {
    assert_error(
      "{'typescript': { 'associations': [1] }}",
      "The 'associations' array in a plugin configuration must contain only strings.",
    );
    assert_error(
      "{'typescript': { 'associations': 1 }}",
      "The 'associations' property in a plugin configuration must be a string or an array of strings.",
    );
    assert_error(
      "{'typescript': { locked: 1 }}",
      "The 'locked' property in a plugin configuration must be a boolean.",
    );
  }

  fn assert_deserializes(text: &str, expected_map: ConfigMap) {
    match deserialize_config(text) {
      Ok(result) => assert_eq!(result, expected_map),
      Err(err) => panic!("Errored, but that was not expected. {}", err),
    }
  }

  fn assert_error(text: &str, expected_err: &str) {
    match deserialize_config(text) {
      Ok(_) => panic!("Did not error, but that was expected."),
      Err(err) => assert_eq!(err.to_string(), expected_err),
    }
  }
}
