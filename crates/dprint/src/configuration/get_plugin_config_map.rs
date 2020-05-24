use std::collections::HashMap;

use crate::plugins::Plugin;
use crate::types::ErrBox;
use super::{ConfigMap, ConfigMapValue};

pub fn get_plugin_config_map(
    plugin: &Box<dyn Plugin>,
    config_map: &mut ConfigMap,
) -> Result<HashMap<String, String>, ErrBox> {
    match get_plugin_config_map_inner(plugin, config_map) {
        Ok(result) => Ok(result),
        Err(err) => err!("Error initializing from configuration file. {}", err.to_string()),
    }
}

fn get_plugin_config_map_inner(
    plugin: &Box<dyn Plugin>,
    config_map: &mut ConfigMap,
) -> Result<HashMap<String, String>, ErrBox> {
    let mut key_name = None;
    let config_keys = plugin.config_keys();
    for config_key in config_keys {
        if config_map.contains_key(config_key) {
            if let Some(key_name) = key_name {
                return err!("Cannot specify both the '{}' and '{}' configurations for {}.", key_name, config_key, plugin.name());
            } else {
                key_name = Some(config_key);
            }
        }
    }

    if let Some(key_name) = key_name {
        let plugin_config_map = config_map.remove(key_name).unwrap();
        if let ConfigMapValue::HashMap(plugin_config_map) = plugin_config_map {
            let mut plugin_config_map = plugin_config_map;
            plugin_config_map.remove("$schema");
            Ok(plugin_config_map)
        } else {
            return err!("Expected the configuration property '{}' to be an object.", key_name);
        }
    } else {
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::plugins::{TestPlugin, Plugin};

    use super::super::{ConfigMapValue, ConfigMap};
    use super::*;

    #[test]
    fn it_should_get_config_for_plugin() {
        let mut config_map = HashMap::new();
        let mut ts_config_map = HashMap::new();
        ts_config_map.insert(String::from("lineWidth"), String::from("40"));
        ts_config_map.insert(String::from("$schema"), String::from("test"));

        config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("80")));
        config_map.insert(String::from("typescript"), ConfigMapValue::HashMap(ts_config_map.clone()));
        let plugin = create_plugin();
        let result = get_plugin_config_map(&(Box::new(plugin) as Box<dyn Plugin>), &mut config_map).unwrap();
        ts_config_map.remove("$schema"); // should not be in result
        assert_eq!(result, ts_config_map.clone());
        assert_eq!(config_map.contains_key("typescript"), false);
    }

    #[test]
    fn it_should_error_when_has_double_plugin_config_keys() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("80")));
        config_map.insert(String::from("typescript"), {
            let mut map = HashMap::new();
            map.insert(String::from("lineWidth"), String::from("40"));
            ConfigMapValue::HashMap(map)
        });
        config_map.insert(String::from("javascript"), {
            let mut map = HashMap::new();
            map.insert(String::from("lineWidth"), String::from("40"));
            ConfigMapValue::HashMap(map)
        });
        assert_errors(
            &mut config_map,
            "Cannot specify both the 'typescript' and 'javascript' configurations for dprint-plugin-typescript.",
        );
    }

    #[test]
    fn it_should_error_plugin_key_is_not_object() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("80")));
        config_map.insert(String::from("typescript"), ConfigMapValue::String(String::from("")));
        assert_errors(
            &mut config_map,
            "Expected the configuration property 'typescript' to be an object.",
        );
    }

    fn assert_errors(config_map: &mut ConfigMap, message: &str) {
        let test_plugin = Box::new(create_plugin()) as Box<dyn Plugin>;
        let result = get_plugin_config_map(&test_plugin, config_map);
        assert_eq!(result.err().unwrap().to_string(), format!("Error initializing from configuration file. {}", message));
    }

    fn create_plugin() -> TestPlugin {
        TestPlugin::new(
            "dprint-plugin-typescript",
            vec!["typescript", "javascript"],
            vec![".ts"]
        )
    }
}
