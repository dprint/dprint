use std::collections::HashMap;
use dprint_core::configuration::GlobalConfiguration;

use crate::environment::Environment;
use crate::types::ErrBox;
use super::{ConfigMap, ConfigMapValue};

pub fn get_global_config(config_map: ConfigMap, environment: &impl Environment) -> Result<GlobalConfiguration, ErrBox> {
    match get_global_config_inner(config_map, environment) {
        Ok(config) => Ok(config),
        Err(err) => err!("Error resolving global config from configuration file. {}", err.to_string()),
    }
}

fn get_global_config_inner(config_map: ConfigMap, environment: &impl Environment) -> Result<GlobalConfiguration, ErrBox> {
    // now get and resolve the global config
    let global_config = get_global_config_from_config_map(config_map)?;
    let global_config_result = dprint_core::configuration::resolve_global_config(global_config);

    // check global diagnostics
    let mut diagnostic_count = 0;
    if !global_config_result.diagnostics.is_empty() {
        for diagnostic in &global_config_result.diagnostics {
            environment.log_error(&diagnostic.message);
            diagnostic_count += 1;
        }
    }

    return if diagnostic_count > 0 {
        err!("Had {} config diagnostic(s).", diagnostic_count)
    } else {
        Ok(global_config_result.config)
    };

    fn get_global_config_from_config_map(config_map: ConfigMap) -> Result<HashMap<String, String>, ErrBox> {
        // at this point, there should only be string values inside the hash map
        let mut global_config = HashMap::new();

        for (key, value) in config_map.into_iter() {
            if key == "$schema" { continue; } // ignore $schema property

            if let ConfigMapValue::String(value) = value {
                global_config.insert(key, value);
            } else {
                return err!("Unexpected non-string property '{}'.", key);
            }
        }

        Ok(global_config)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::configuration::ConfigMap;
    use crate::environment::TestEnvironment;
    use super::*;

    #[test]
    fn it_should_get_global_config() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("80")));
        assert_gets(config_map, GlobalConfiguration {
            line_width: Some(80),
            use_tabs: None,
            indent_width: None,
            new_line_kind: None,
        });
    }

    #[test]
    fn it_should_error_on_unexpected_object_properties() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("test"), ConfigMapValue::HashMap(HashMap::new()));
        assert_errors(
            config_map,
            vec![],
            "Unexpected non-string property 'test'.",
        );
    }

    #[test]
    fn it_should_log_config_file_diagnostics() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("test")));
        config_map.insert(String::from("unknownProperty"), ConfigMapValue::String(String::from("80")));
        assert_errors(
            config_map,
            vec![
                "Error parsing configuration value for 'lineWidth'. Message: invalid digit found in string",
                "Unknown property in configuration: unknownProperty"
            ],
            "Had 2 config diagnostic(s).",
        );
    }

    #[test]
    fn it_should_ignore_schema_property() {
        let mut config_map = HashMap::new();
        config_map.insert(String::from("$schema"), ConfigMapValue::String(String::from("test")));
        assert_gets(config_map, GlobalConfiguration {
            line_width: None,
            use_tabs: None,
            indent_width: None,
            new_line_kind: None,
        });
    }

    fn assert_gets(config_map: ConfigMap, global_config: GlobalConfiguration) {
        let test_environment = TestEnvironment::new();
        let result = get_global_config(config_map, &test_environment).unwrap();
        assert_eq!(result, global_config);
    }

    fn assert_errors(config_map: ConfigMap, logged_errors: Vec<&'static str>, message: &str) {
        let test_environment = TestEnvironment::new();
        let result = get_global_config(config_map, &test_environment);
        assert_eq!(result.err().unwrap().to_string(), format!("Error resolving global config from configuration file. {}", message));
        assert_eq!(test_environment.get_logged_errors(), logged_errors);
    }
}