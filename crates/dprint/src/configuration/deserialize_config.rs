use std::collections::HashMap;
use jsonc_parser::{JsonValue, JsonArray, JsonObject};
use dprint_core::types::ErrBox;
use dprint_core::configuration::{ConfigKeyMap, ConfigKeyValue};
use super::{ConfigMapValue, ConfigMap};

pub fn deserialize_config(config_file_text: &str) -> Result<ConfigMap, ErrBox> {
    let value = jsonc_parser::parse_to_value(&config_file_text)?;

    let root_object_node = match value {
        Some(JsonValue::Object(obj)) => obj,
        _ => return err!("Expected a root object in the json"),
    };

    let mut properties = HashMap::new();

    for (key, value) in root_object_node.into_iter() {
        let property_name = key;
        let property_value = match value {
            JsonValue::Object(obj) => ConfigMapValue::HashMap(json_obj_to_hash_map(&property_name, obj)?),
            JsonValue::Array(arr) => ConfigMapValue::Vec(json_array_to_vec(&property_name, arr)?),
            JsonValue::Boolean(value) => ConfigMapValue::from_bool(value),
            JsonValue::String(value) => ConfigMapValue::KeyValue(ConfigKeyValue::String(value.into_owned())),
            JsonValue::Number(value) => ConfigMapValue::from_i32(match value.parse::<i32>() {
                Ok(value) => value,
                Err(err) => return err!(
                    "Expected property '{}' with value '{}' to be convertable to a signed integer. {}",
                    property_name,
                    value,
                    err.to_string()
                ),
            }),
            _ => return err!("Expected an object, boolean, string, or number in root object property '{}'", property_name),
        };
        properties.insert(property_name, property_value);
    }

    Ok(properties)
}

fn json_obj_to_hash_map(parent_prop_name: &str, obj: JsonObject) -> Result<ConfigKeyMap, ErrBox> {
    let mut properties = HashMap::new();

    for (key, value) in obj.into_iter() {
        let property_name = key;
        let property_value = match value_to_plugin_config_key_value(value) {
            Ok(result) => result,
            Err(err) => return err!("{} in object property '{} -> {}'", err, parent_prop_name, property_name),
        };
        properties.insert(property_name, property_value);
    }

    Ok(properties)
}

fn json_array_to_vec(parent_prop_name: &str, array: JsonArray) -> Result<Vec<String>, ErrBox> {
    let mut elements = Vec::new();

    for element in array.into_iter() {
        let value = match value_to_string(element) {
            Ok(result) => result,
            Err(err) => return err!("{} in array '{}'", err, parent_prop_name),
        };
        elements.push(value);
    }

    Ok(elements)
}

fn value_to_string(value: JsonValue) -> Result<String, ErrBox> {
    match value {
        JsonValue::String(value) => Ok(value.into_owned()),
        _ => return err!("Expected a string"),
    }
}

fn value_to_plugin_config_key_value(value: JsonValue) -> Result<ConfigKeyValue, ErrBox> {
    Ok(match value {
        JsonValue::Boolean(value) => ConfigKeyValue::Bool(value),
        JsonValue::String(value) => ConfigKeyValue::String(value.into_owned()),
        JsonValue::Number(value) => ConfigKeyValue::Number(value.parse::<i32>()?),
        _ => return err!("Expected a boolean, string, or number"),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use dprint_core::configuration::{ConfigKeyValue};
    use super::deserialize_config;
    use super::super::{ConfigMapValue, ConfigMap};

    #[test]
    fn it_should_error_when_there_is_a_parser_error() {
        assert_error("{prop}", "Unexpected token on line 1 column 2.");
    }

    #[test]
    fn it_should_error_when_no_object_in_root() {
        assert_error("[]", "Expected a root object in the json");
    }

    #[test]
    fn it_should_error_when_the_root_property_has_an_unexpected_value_type() {
        assert_error("{'prop': null}", "Expected an object, boolean, string, or number in root object property 'prop'");
    }

    #[test]
    fn it_should_error_when_the_sub_object_has_object() {
        assert_error("{'prop': { 'test': {}}}", "Expected a boolean, string, or number in object property 'prop -> test'");
    }

    #[test]
    fn it_should_deserialize_empty_object() {
        assert_deserializes("{}", HashMap::new());
    }

    #[test]
    fn it_should_deserialize_full_object() {
        let mut expected_props = HashMap::new();
        expected_props.insert(String::from("includes"), ConfigMapValue::Vec(Vec::new()));
        let mut ts_hash_map = HashMap::new();
        ts_hash_map.insert(String::from("lineWidth"), ConfigKeyValue::from_i32(40));
        ts_hash_map.insert(String::from("preferSingleLine"), ConfigKeyValue::from_bool(true));
        ts_hash_map.insert(String::from("other"), ConfigKeyValue::from_str("test"));
        expected_props.insert(String::from("typescript"), ConfigMapValue::HashMap(ts_hash_map));
        assert_deserializes(
            "{'includes': [], 'typescript': { 'lineWidth': 40, 'preferSingleLine': true, 'other': 'test' }}",
            expected_props
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
