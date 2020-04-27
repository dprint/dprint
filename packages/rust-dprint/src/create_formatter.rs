use dprint_core::plugins::Formatter;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use super::configuration::{self, StringOrHashMap};
use super::environment::Environment;

pub fn create_formatter(config_path: Option<&str>, environment: &impl Environment) -> Result<Formatter, String> {
    let mut plugins = Formatter::new(vec![
        Box::new(dprint_plugin_typescript::TypeScriptPlugin::new()),
        Box::new(dprint_plugin_jsonc::JsoncPlugin::new())
    ]);

    match initialize_plugins(config_path, &mut plugins, environment) {
        Ok(()) => Ok(plugins),
        Err(err) => Err(format!("Error initializing from the configuration file: {}", err)),
    }
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
            environment.log_error(&diagnostic.message);
            diagnostic_count += 1;
        }
    }

    if diagnostic_count > 0 {
        Err(format!("Had {} configuration file diagnostic(s).", diagnostic_count))
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
            return Err(format!("Unexpected object property '{}' in config file.", key));
        }
    }

    Ok(global_config)
}

fn deserialize_config_file(config_path: Option<&str>, environment: &impl Environment) -> Result<HashMap<String, StringOrHashMap>, String> {
    if let Some(config_path) = config_path {
        let config_path = Path::new(config_path);
        let canonical_file_path = config_path.canonicalize().map(|f| String::from(f.to_string_lossy())).unwrap_or(String::from(config_path.to_string_lossy()));
        let config_file_text = match environment.read_file(&PathBuf::from(config_path)) {
            Ok(contents) => contents,
            Err(e) => return Err(format!("Could not read config file {} at {}", canonical_file_path, e.to_string())),
        };

        let result = match configuration::deserialize_config(&config_file_text) {
            Ok(map) => map,
            Err(e) => return Err(format!("Error deserializing config file '{}'. {}", canonical_file_path, e.to_string())),
        };

        Ok(result)
    } else {
        Ok(HashMap::new())
    }
}
