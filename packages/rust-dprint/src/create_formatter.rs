use dprint_core::plugins::{Formatter, Plugin};
use std::collections::HashMap;
use std::path::PathBuf;

use super::configuration::{self, StringOrHashMap};
use super::environment::Environment;

pub fn create_formatter(config_path: Option<&str>, environment: &impl Environment) -> Result<Formatter, String> {
    let mut plugins = Formatter::new(get_uninitialized_plugins());

    match initialize_plugins(config_path, &mut plugins, environment) {
        Ok(()) => Ok(plugins),
        Err(err) => {
            let canonical_file_path = config_path.and_then(|x| PathBuf::from(x).canonicalize().ok()).map(|f| String::from(f.to_string_lossy()));
            if let Some(canonical_file_path) = canonical_file_path {
                Err(format!("Error initializing from configuration file '{}'. {}", canonical_file_path, err))
            } else {
                Err(format!("Error initializing from configuration file. {}", err))
            }
        },
    }
}

pub fn get_uninitialized_plugins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(dprint_plugin_typescript::TypeScriptPlugin::new()),
        Box::new(dprint_plugin_jsonc::JsoncPlugin::new())
    ]
}

fn initialize_plugins(config_path: Option<&str>, formatter: &mut Formatter, environment: &impl Environment) -> Result<(), String> {
    let mut config_map = deserialize_config_file(config_path, environment)?;

    // check for the project type diagnostic
    if !config_map.is_empty() {
        if let Some(diagnostic) = configuration::handle_project_type_diagnostic(&mut config_map) {
            environment.log_error(&diagnostic.message);
        }
    }

    // get hashmaps per plugin
    let mut plugins_to_config = handle_plugins_to_config_map(&formatter, &mut config_map)?;

    // now get and resolve the global config
    let global_config = get_global_config_from_config_map(config_map)?;
    let global_config_result = dprint_core::configuration::resolve_global_config(&global_config);

    // check global diagnostics
    let mut diagnostic_count = 0;
    if !global_config_result.diagnostics.is_empty() {
        for diagnostic in &global_config_result.diagnostics {
            environment.log_error(&diagnostic.message);
            diagnostic_count += 1;
        }
    }

    // intiailize the plugins
    for plugin in formatter.iter_plugins_mut() {
        plugin.initialize(plugins_to_config.remove(&plugin.name()).unwrap_or(HashMap::new()), &global_config_result.config);

        for diagnostic in plugin.get_configuration_diagnostics() {
            environment.log_error(&format!("[{}]: {}", plugin.name(), diagnostic.message));
            diagnostic_count += 1;
        }
    }

    if diagnostic_count > 0 {
        Err(format!("Had {} diagnostic(s).", diagnostic_count))
    } else {
        Ok(())
    }
}

fn handle_plugins_to_config_map(
    formatter: &Formatter,
    config_map: &mut HashMap<String, StringOrHashMap>,
) -> Result<HashMap<&'static str, HashMap<String, String>>, String> {
    let mut plugin_maps = HashMap::new();
    for plugin in formatter.iter_plugins() {
        let mut key_name = None;
        let config_keys = plugin.config_keys();
        for config_key in config_keys {
            if config_map.contains_key(&config_key) {
                if let Some(key_name) = key_name {
                    return Err(format!("Cannot specify both the '{}' and '{}' configurations for {}.", key_name, config_key, plugin.name()));
                } else {
                    key_name = Some(config_key);
                }
            }
        }
        if let Some(key_name) = key_name {
            let plugin_config_map = config_map.remove(&key_name).unwrap();
            if let StringOrHashMap::HashMap(plugin_config_map) = plugin_config_map {
                plugin_maps.insert(plugin.name(), plugin_config_map);
            } else {
                return Err(format!("Expected the configuration property '{}' to be an object.", key_name));
            }
        }
    }
    Ok(plugin_maps)
}

fn get_global_config_from_config_map(
    config_map: HashMap<String, StringOrHashMap>,
) -> Result<HashMap<String, String>, String> {
    // at this point, there should only be string values inside the hash map
    let mut global_config = HashMap::new();

    for (key, value) in config_map.into_iter() {
        if let StringOrHashMap::String(value) = value {
            global_config.insert(key, value);
        } else {
            return Err(format!("Unexpected object property '{}'.", key));
        }
    }

    Ok(global_config)
}

fn deserialize_config_file(config_path: Option<&str>, environment: &impl Environment) -> Result<HashMap<String, StringOrHashMap>, String> {
    if let Some(config_path) = config_path {
        let config_file_text = match environment.read_file(&PathBuf::from(config_path)) {
            Ok(contents) => contents,
            Err(e) => return Err(e.to_string()),
        };

        let result = match configuration::deserialize_config(&config_file_text) {
            Ok(map) => map,
            Err(e) => return Err(format!("Error deserializing. {}", e.to_string())),
        };

        Ok(result)
    } else {
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::create_formatter;
    use super::super::environment::{Environment, TestEnvironment};

    #[test]
    fn it_should_get_formatter() {
        assert_creates(r#"{
    "projectType": "openSource",
    "lineWidth": "80",
    "typescript": {
        "lineWidth": 40
    },
    "jsonc": {
        "lineWidth": 70
    }
}"#);
    }

    #[test]
    fn it_should_only_warn_when_missing_project_type() {
        let test_environment = TestEnvironment::new();
        let cfg_file_path = "./config.json";
        test_environment.write_file(&PathBuf::from(cfg_file_path), r#"{ "lineWidth": 40 }"#).unwrap();
        let result = create_formatter(Some(cfg_file_path), &test_environment);
        assert_eq!(result.is_ok(), true);
        assert_eq!(test_environment.get_logged_errors()[0].find("The 'projectType' property").is_some(), true);
    }

    #[test]
    fn it_should_error_when_has_double_plugin_config_keys() {
        assert_errors(r#"{
    "projectType": "openSource",
    "lineWidth": "80",
    "typescript": {
        "lineWidth": 40
    },
    "javascript": {
        "lineWidth": 70
    }
}"#, vec![], "Error initializing from configuration file. Cannot specify both the 'typescript' and 'javascript' configurations for dprint-plugin-typescript.");
    }

    #[test]
    fn it_should_error_plugin_key_is_not_object() {
        assert_errors(r#"{
    "projectType": "openSource",
    "typescript": ""
}"#, vec![], "Error initializing from configuration file. Expected the configuration property 'typescript' to be an object.");
    }

    #[test]
    fn it_should_log_global_diagnostics() {
        assert_errors(r#"{
    "projectType": "openSource",
    "lineWidth": "null"
}"#, vec!["Error parsing configuration value for 'lineWidth'. Message: invalid digit found in string"], "Error initializing from configuration file. Had 1 diagnostic(s).");
    }


    #[test]
    fn it_should_log_unexpected_object_properties() {
        assert_errors(r#"{
    "projectType": "openSource",
    "test": {}
}"#, vec![], "Error initializing from configuration file. Unexpected object property 'test'.");
    }

    #[test]
    fn it_should_log_plugin_diagnostics() {
        assert_errors(
            r#"{
    "projectType": "openSource",
    "typescript": {
        "lineWidth": "null"
    }
}"#,
            vec!["[dprint-plugin-typescript]: Error parsing configuration value for 'lineWidth'. Message: invalid digit found in string"],
            "Error initializing from configuration file. Had 1 diagnostic(s)."
        );
    }

    #[test]
    fn it_should_error_when_cannot_parse() {
        assert_errors(
            r#"{"#,
            vec![],
            "Error initializing from configuration file. Error deserializing. Unterminated object on line 1 column 1."
        );
    }

    #[test]
    fn it_should_error_when_no_file() {
        let test_environment = TestEnvironment::new();
        let cfg_file_path = "./config.json";
        let result = create_formatter(Some(cfg_file_path), &test_environment);
        assert_eq!(result.err().unwrap(), "Error initializing from configuration file. Could not find file at path ./config.json");
    }

    fn assert_creates(cfg_file_text: &str) {
        let test_environment = TestEnvironment::new();
        let cfg_file_path = "./config.json";
        test_environment.write_file(&PathBuf::from(cfg_file_path), cfg_file_text).unwrap();
        assert_eq!(create_formatter(Some(cfg_file_path), &test_environment).is_ok(), true);
    }

    fn assert_errors(cfg_file_text: &str, logged_errors: Vec<&'static str>, message: &str) {
        let test_environment = TestEnvironment::new();
        let cfg_file_path = "./config.json";
        test_environment.write_file(&PathBuf::from(cfg_file_path), cfg_file_text).unwrap();
        let result = create_formatter(Some(cfg_file_path), &test_environment);
        assert_eq!(result.err().unwrap(), message);
        assert_eq!(test_environment.get_logged_errors(), logged_errors);
    }
}
