use std::collections::HashMap;
use jsonc_parser::{JsonValue, JsonArray, JsonObject};
use super::{ConfigMapValue, ConfigMap};

pub fn deserialize_config(config_file_text: &str) -> Result<ConfigMap, String> {
    let value = match jsonc_parser::parse_to_value(&config_file_text) {
        Ok(c) => c,
        Err(e) => return Err(e.get_message_with_range(&config_file_text)),
    };

    let root_object_node = match value {
        Some(JsonValue::Object(obj)) => obj,
        _ => return Err(String::from("Expected a root object in the json")),
    };

    let mut properties = HashMap::new();

    for (key, value) in root_object_node.into_iter() {
        let property_name = key;
        let property_value = match value {
            JsonValue::Object(obj) => ConfigMapValue::HashMap(json_obj_to_hash_map(&property_name, obj)?),
            JsonValue::Array(arr) => ConfigMapValue::Vec(json_array_to_vec(&property_name, arr)?),
            JsonValue::Boolean(value) => ConfigMapValue::String(value.to_string()),
            JsonValue::String(value) => ConfigMapValue::String(value),
            JsonValue::Number(value) => ConfigMapValue::String(value),
            _ => return Err(format!("Expected an object, boolean, string, or number in root object property '{}'", property_name)),
        };
        properties.insert(property_name, property_value);
    }

    Ok(properties)
}

fn json_obj_to_hash_map(parent_prop_name: &str, obj: JsonObject) -> Result<HashMap<String, String>, String> {
    let mut properties = HashMap::new();

    for (key, value) in obj.into_iter() {
        let property_name = key;
        let property_value = match value_to_string(value) {
            Ok(result) => result,
            Err(err) => return Err(format!("{} in object property '{} -> {}'", err, parent_prop_name, property_name)),
        };
        properties.insert(property_name, property_value);
    }

    Ok(properties)
}

fn json_array_to_vec(parent_prop_name: &str, array: JsonArray) -> Result<Vec<String>, String> {
    let mut elements = Vec::new();

    for element in array.into_iter() {
        let value = match value_to_string(element) {
            Ok(result) => result,
            Err(err) => return Err(format!("{} in array '{}'", err, parent_prop_name)),
        };
        elements.push(value);
    }

    Ok(elements)
}

fn value_to_string(value: JsonValue) -> Result<String, String> {
    match value {
        JsonValue::Boolean(value) => Ok(value.to_string()),
        JsonValue::String(value) | JsonValue::Number(value) => Ok(value),
        _ => return Err(String::from("Expected a boolean, string, or number")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
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
        expected_props.insert(String::from("projectType"), ConfigMapValue::String(String::from("openSource")));
        let mut ts_hash_map = HashMap::new();
        ts_hash_map.insert(String::from("lineWidth"), String::from("40"));
        ts_hash_map.insert(String::from("preferSingleLine"), String::from("true"));
        ts_hash_map.insert(String::from("other"), String::from("test"));
        expected_props.insert(String::from("typescript"), ConfigMapValue::HashMap(ts_hash_map));
        assert_deserializes(
            "{'projectType': 'openSource', 'typescript': { 'lineWidth': 40, 'preferSingleLine': true, 'other': 'test' }}",
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
            Err(err) => assert_eq!(err, expected_err),
        }
    }
}
