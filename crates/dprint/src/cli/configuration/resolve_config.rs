use std::path::PathBuf;
use std::collections::HashMap;
use colored::Colorize;
use crate::cache::Cache;
use crate::configuration::{ConfigMap, ConfigMapValue, deserialize_config};
use crate::cli::CliArgs;
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::{PluginSourceReference, parse_plugin_source_reference};
use crate::utils::{ResolvedPath, resolve_url_or_file_path, PathSource};

use super::resolve_main_config_path;

pub struct ResolvedConfig {
    pub resolved_path: ResolvedPath,
    /// The folder that should be considered the "root".
    pub base_path: PathBuf,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub plugins: Vec<PluginSourceReference>,
    pub incremental: bool,
    pub project_type: Option<String>,
    pub config_map: ConfigMap,
}

pub async fn resolve_config_from_args<TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedConfig, ErrBox> {
    let resolved_config_path = resolve_main_config_path(args, cache, environment).await?;
    let base_source = resolved_config_path.resolved_path.source.parent();
    let config_file_path = &resolved_config_path.resolved_path.file_path;
    let main_config_map = get_config_map_from_path(config_file_path, environment)?;

    let mut main_config_map = match main_config_map {
        Ok(main_config_map) => main_config_map,
        Err(err) => {
            // allow no config file when plugins are specified
            if !args.plugins.is_empty() && !environment.path_exists(config_file_path) {
                let mut config_map = HashMap::new();
                // hack: easy way to supress project type diagnostic check
                config_map.insert(String::from("projectType"), ConfigMapValue::String(String::from("openSource")));
                config_map
            } else {
                return err!(
                    "No config file found at {}. Did you mean to create (dprint init) or specify one (--config <path>)?\n  Error: {}",
                    config_file_path.display(),
                    err.to_string(),
                )
            }
        }
    };

    let plugins_vec = take_plugins_array_from_config_map(&mut main_config_map, &base_source)?; // always take this out of the config map
    let plugins = if args.plugins.is_empty() {
        // filter out any non-wasm plugins from remote config
        if !resolved_config_path.resolved_path.is_local() {
            filter_non_wasm_plugins(plugins_vec, environment) // NEVER REMOVE THIS STATEMENT
        } else {
            plugins_vec
        }
    } else {
        let base_path = PathSource::new_local(resolved_config_path.base_path.clone());
        let mut plugins = Vec::with_capacity(args.plugins.len());
        for url_or_file_path in args.plugins.iter() {
            plugins.push(parse_plugin_source_reference(url_or_file_path, &base_path)?);
        }

        plugins
    };

    // IMPORTANT
    // =========
    // Remove the includes and excludes from remote configuration since
    // we don't want it specifying something like system or some configuration
    // files that it could change. Basically, the end user should have 100%
    // control over what files get formatted.
    if !resolved_config_path.resolved_path.is_local() {
        // Careful! Don't be fancy and ensure both of these are removed.
        let removed_includes = main_config_map.remove("includes").is_some(); // NEVER REMOVE THIS STATEMENT
        let removed_excludes = main_config_map.remove("excludes").is_some(); // NEVER REMOVE THIS STATEMENT
        let was_removed = removed_includes || removed_excludes;
        if was_removed && resolved_config_path.resolved_path.is_first_download {
            environment.log(&get_warn_includes_excludes_message());
        }
    }
    // =========

    let includes = take_array_from_config_map(&mut main_config_map, "includes")?;
    let excludes = take_array_from_config_map(&mut main_config_map, "excludes")?;
    let incremental = take_bool_from_config_map(&mut main_config_map, "incremental", false)?;
    let project_type = match main_config_map.remove("projectType") {
        Some(ConfigMapValue::String(project_type)) => Some(project_type),
        None => None,
        _ => return err!("The projectType configuration should be a string."),
    };
    let extends = take_extends(&mut main_config_map)?;
    let mut resolved_config = ResolvedConfig {
        resolved_path: resolved_config_path.resolved_path,
        base_path: resolved_config_path.base_path,
        config_map: main_config_map,
        includes,
        excludes,
        plugins,
        incremental,
        project_type,
    };

    // resolve extends
    resolve_extends(&mut resolved_config, extends, &base_source, cache, environment).await?;
    remove_locked_properties(&mut resolved_config);

    Ok(resolved_config)
}

async fn resolve_extends<TEnvironment : Environment>(
    resolved_config: &mut ResolvedConfig,
    extends: Vec<String>,
    base_path: &PathSource,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<(), ErrBox> {
    // Rust does not support async recursion without boxing, so this is done to avoid having to do recursion
    let mut extends_items = extends.into_iter().map(|url_or_file_path| (base_path.clone(), url_or_file_path)).collect::<Vec<_>>();

    let mut index = 0;
    loop {
        let (base_path, url_or_file_path) = match extends_items.get(index) {
            Some(extends_item) => extends_item,
            None => return Ok(()), // all done
        };

        let resolved_path = resolve_url_or_file_path(&url_or_file_path, base_path, cache, environment).await?;
        let extends = match handle_config_file(&resolved_path, resolved_config, environment).await {
            Ok(extends) => extends,
            Err(err) => return err!("Error with '{}'. {}", resolved_path.source.display(), err.to_string())
        };

        // push the current extends onto the extends items
        let parent = resolved_path.source.parent();
        extends_items.splice(index + 1..index + 1, extends.into_iter().map(|url_or_file_path| (parent.clone(), url_or_file_path)));

        index += 1;
    }
}

async fn handle_config_file<'a, TEnvironment : Environment>(
    resolved_path: &ResolvedPath,
    resolved_config: &mut ResolvedConfig,
    environment: &TEnvironment,
) -> Result<Vec<String>, ErrBox> {
    let config_file_path = &resolved_path.file_path;
    let mut new_config_map = match get_config_map_from_path(config_file_path, environment)? {
        Ok(config_map) => config_map,
        Err(err) => return Err(err),
    };
    let extends = take_extends(&mut new_config_map)?;

    // Discard any properties that shouldn't be inherited
    new_config_map.remove("projectType");
    // IMPORTANT
    // =========
    // Remove the includes and excludes from all referenced configuration since
    // we don't want it specifying something like system or some configuration
    // files that it could change. Basically, the end user should have 100%
    // control over what files get formatted.
    new_config_map.remove("includes"); // NEVER REMOVE THIS STATEMENT
    new_config_map.remove("excludes"); // NEVER REMOVE THIS STATEMENT
    // Also remove any non-wasm plugins, but only for remote configurations.
    // The assumption here is that the user won't be malicious to themselves.
    let plugins = take_plugins_array_from_config_map(&mut new_config_map, &resolved_path.source.parent())?;
    let plugins = if !resolved_path.is_local() { filter_non_wasm_plugins(plugins, environment) } else { plugins };
    // =========

    // combine plugins
    resolved_config.plugins.extend(plugins);

    for (key, value) in new_config_map {
        match value {
            ConfigMapValue::String(text) => {
                if !resolved_config.config_map.contains_key(&key) {
                    resolved_config.config_map.insert(key, ConfigMapValue::String(text));
                }
            },
            ConfigMapValue::Vec(items) => {
                if !resolved_config.config_map.contains_key(&key) {
                    resolved_config.config_map.insert(key, ConfigMapValue::Vec(items));
                }
            },
            ConfigMapValue::HashMap(obj) => {
                if let Some(resolved_config_obj) = resolved_config.config_map.get_mut(&key) {
                    match resolved_config_obj {
                        ConfigMapValue::HashMap(resolved_config_obj) => {
                            // check for locked configuration
                            if let Some(is_locked) = obj.get("locked") {
                                if is_locked.to_lowercase() == "true" && !resolved_config_obj.is_empty() {
                                    return err!(
                                        concat!(
                                            "The configuration for \"{}\" was locked, but a parent configuration specified it. ",
                                            "Locked configurations cannot have their properties overridden."
                                        ),
                                        key
                                    );
                                }
                            }

                            for (key, value) in obj {
                                if !resolved_config_obj.contains_key(&key) {
                                    resolved_config_obj.insert(key, value);
                                }
                            }
                        },
                        _ => {
                            // ignore...
                        }
                    }
                } else {
                    resolved_config.config_map.insert(key, ConfigMapValue::HashMap(obj));
                }
            }
        }
    }

    Ok(extends)
}

fn take_extends(config_map: &mut ConfigMap) -> Result<Vec<String>, ErrBox> {
    match config_map.remove("extends") {
        Some(ConfigMapValue::String(url_or_file_path)) => Ok(vec![url_or_file_path]),
        Some(ConfigMapValue::Vec(url_or_file_paths)) => Ok(url_or_file_paths),
        Some(_) => return err!("Extends in configuration must be a string or an array of strings."),
        None => Ok(Vec::new())
    }
}

fn get_config_map_from_path(file_path: &PathBuf, environment: &impl Environment) -> Result<Result<ConfigMap, ErrBox>, ErrBox> {
    let config_file_text = match environment.read_file(file_path) {
        Ok(file_text) => file_text,
        Err(err) => return Ok(Err(err)),
    };

    let result = match deserialize_config(&config_file_text) {
        Ok(map) => map,
        Err(e) => return err!("Error deserializing. {}", e.to_string()),
    };

    Ok(Ok(result))
}

fn take_plugins_array_from_config_map(config_map: &mut ConfigMap, base_path: &PathSource) -> Result<Vec<PluginSourceReference>, ErrBox> {
    let plugin_url_or_file_paths = take_array_from_config_map(config_map, "plugins")?;
    let mut plugins = Vec::with_capacity(plugin_url_or_file_paths.len());
    for url_or_file_path in plugin_url_or_file_paths {
        plugins.push(parse_plugin_source_reference(&url_or_file_path, base_path)?);
    }
    Ok(plugins)
}

fn take_array_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Vec<String>, ErrBox> {
    let mut result = Vec::new();
    if let Some(value) = config_map.remove(property_name) {
        match value {
            ConfigMapValue::Vec(elements) => {
                result.extend(elements);
            },
            _ => return err!("Expected array in '{}' property.", property_name),
        }
    }
    Ok(result)
}

fn take_bool_from_config_map(config_map: &mut ConfigMap, property_name: &str, default_value: bool) -> Result<bool, ErrBox> {
    let mut result = default_value;
    if let Some(value) = config_map.remove(property_name) {
        match value {
            ConfigMapValue::String(value) => {
                if value.to_lowercase() == "true" {
                    result = true;
                } else if value.to_lowercase() == "false" {
                    result = false;
                } else {
                    return err!("Expected boolean in '{}' property.", property_name);
                }
            },
            _ => return err!("Expected boolean in '{}' property.", property_name),
        }
    }
    Ok(result)
}

fn filter_non_wasm_plugins(plugins: Vec<PluginSourceReference>, environment: &impl Environment) -> Vec<PluginSourceReference> {
    if plugins.iter().any(|plugin| !plugin.is_wasm_plugin()) {
        environment.log(&get_warn_non_wasm_plugins_message());
        plugins.into_iter().filter(|plugin| plugin.is_wasm_plugin()).collect()
    } else {
        plugins
    }
}

fn get_warn_includes_excludes_message() -> String {
    format!(
        "{} The 'includes' and 'excludes' properties are ignored for security reasons on remote configuration.",
        "Note: ".bold().to_string()
    )
}

fn get_warn_non_wasm_plugins_message() -> String {
    format!(
        "{} Non-wasm plugins are ignored for security reasons on remote configuration.",
        "Note: ".bold().to_string()
    )
}

fn remove_locked_properties(resolved_config: &mut ResolvedConfig) {
    // Remove this property on each sub configuration as it's not useful
    // for the caller to know about.
    for (_, value) in resolved_config.config_map.iter_mut() {
        if let ConfigMapValue::HashMap(obj) = value {
            obj.remove("locked");
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use crate::cache::Cache;
    use crate::cli::{parse_args, TestStdInReader};
    use crate::environment::{Environment, TestEnvironment};
    use crate::types::ErrBox;

    use super::*;

    async fn get_result(url: &str, environment: &impl Environment) -> Result<ResolvedConfig, ErrBox> {
        let stdin_reader = TestStdInReader::new();
        let args = parse_args(vec![String::from(""), String::from("-c"), String::from(url)], &stdin_reader).unwrap();
        let cache = Cache::new(environment.to_owned()).unwrap();
        resolve_config_from_args(&args, &cache, &environment).await
    }

    #[tokio::test]
    async fn it_should_get_local_config_file() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": ["test"],
            "excludes": ["test"]
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.base_path, PathBuf::from("./"));
        assert_eq!(result.resolved_path.is_local(), true);
        assert_eq!(result.project_type, Some(String::from("openSource")));
        assert_eq!(result.config_map.contains_key("includes"), false);
        assert_eq!(result.config_map.contains_key("excludes"), false);
        assert_eq!(result.includes, vec!["test"]);
        assert_eq!(result.excludes, vec!["test"]);
    }

    #[tokio::test]
    async fn it_should_get_remote_config_file() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.base_path, PathBuf::from("./"));
        assert_eq!(result.resolved_path.is_remote(), true);
        assert_eq!(result.project_type, Some(String::from("openSource")));
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_includes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": ["test"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.includes.len(), 0);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.includes.len(), 0);
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "excludes": ["test"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.excludes.len(), 0);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.excludes.len(), 0);
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_includes_and_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": [],
            "excludes": []
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.includes.len(), 0);
        assert_eq!(result.excludes.len(), 0);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.includes.len(), 0);
        assert_eq!(result.excludes.len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_warn_remove_config_no_includes_or_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#.as_bytes());

        get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
    }

    #[tokio::test]
    async fn it_should_handle_single_extends() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin2.wasm"],
            "lineWidth": 4,
            "otherProp": { "test": 4 }, // should ignore
            "otherProp2": "a",
            "test": {
                "prop": 6,
                "other": "test"
            },
            "test2": {
                "prop": 2
            },
            "includes": ["test"],
            "excludes": ["test"]
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/test.json",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "otherProp": 6,
            "test": {
                "prop": 5
            },
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.base_path, PathBuf::from("./"));
        assert_eq!(result.resolved_path.is_local(), true);
        assert_eq!(result.project_type, None); // should always come from main config
        assert_eq!(result.includes.len(), 0);
        assert_eq!(result.excludes.len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
        ]);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("otherProp"), ConfigMapValue::String(String::from("6")));
        expected_config_map.insert(String::from("otherProp2"), ConfigMapValue::String(String::from("a")));
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("5"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));
        expected_config_map.insert(String::from("test2"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("2"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_array_extends() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin2.wasm"],
            "lineWidth": 4,
            "otherProp": 6,
            "test": {
                "prop": 6,
                "other": "test"
            },
            "test2": {
                "prop": 2
            }
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/test2.json", r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin3.wasm"],
            "otherProp": 7,
            "asdf": 4,
            "test": {
                "other": "test2"
            }
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": [
                "https://dprint.dev/test.json",
                "https://dprint.dev/test2.json",
            ],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "test": {
                "prop": 5
            },
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.project_type, None);
        assert_eq!(result.includes.len(), 0);
        assert_eq!(result.excludes.len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin3.wasm"),
        ]);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("otherProp"), ConfigMapValue::String(String::from("6")));
        expected_config_map.insert(String::from("asdf"), ConfigMapValue::String(String::from("4")));
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("5"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));
        expected_config_map.insert(String::from("test2"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("2"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_extends_within_an_extends() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "extends": "https://dprint.dev/test2.json",
            "plugins": ["https://plugins.dprint.dev/test-plugin2.wasm"],
            "lineWidth": 4,
            "otherProp": 6,
            "test": {
                "prop": 6,
                "other": "test"
            },
            "test2": {
                "prop": 2
            }
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/test2.json", r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin3.wasm"],
            "otherProp": 7,
            "asdf": 4,
            "test": {
                "other": "test2"
            }
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/test3.json", r#"{
            "asdf": 4,
            "newProp": "test"
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": [
                "https://dprint.dev/test.json"
                "https://dprint.dev/test3.json" // should have lowest precedence
            ],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "test": {
                "prop": 5
            },
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.project_type, None);
        assert_eq!(result.includes.len(), 0);
        assert_eq!(result.excludes.len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
            PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin3.wasm"),
        ]);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("lineWidth"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("otherProp"), ConfigMapValue::String(String::from("6")));
        expected_config_map.insert(String::from("asdf"), ConfigMapValue::String(String::from("4")));
        expected_config_map.insert(String::from("newProp"), ConfigMapValue::String(String::from("test")));
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("5"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));
        expected_config_map.insert(String::from("test2"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("2"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_relative_remote_extends() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/dir/test.json", r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/otherDir/test.json", r#"{
            "extends": "https://test.dprint.dev/test.json",
            "prop3": 3
        }"#.as_bytes());
        environment.add_remote_file("https://test.dprint.dev/test.json", r#"{
            "extends": [
                "other.json",
                "dir/test.json"
            ],
            "prop4": 4,
        }"#.as_bytes());
        environment.add_remote_file("https://test.dprint.dev/other.json", r#"{
            "prop5": 5,
        }"#.as_bytes());
        environment.add_remote_file("https://test.dprint.dev/dir/test.json", r#"{
            "prop6": 6,
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("prop1"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("prop2"), ConfigMapValue::String(String::from("2")));
        expected_config_map.insert(String::from("prop3"), ConfigMapValue::String(String::from("3")));
        expected_config_map.insert(String::from("prop4"), ConfigMapValue::String(String::from("4")));
        expected_config_map.insert(String::from("prop5"), ConfigMapValue::String(String::from("5")));
        expected_config_map.insert(String::from("prop6"), ConfigMapValue::String(String::from("6")));
        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_remote_in_local_extends() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/dir/test.json",
            "prop1": 1
        }"#).unwrap();
        environment.add_remote_file("https://dprint.dev/dir/test.json", r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/otherDir/test.json", r#"{
            "prop3": 3
        }"#.as_bytes());

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("prop1"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("prop2"), ConfigMapValue::String(String::from("2")));
        expected_config_map.insert(String::from("prop3"), ConfigMapValue::String(String::from("3")));
        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_relative_local_extends() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/dir/test.json"), r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/otherDir/test.json"), r#"{
            "prop3": 3
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);

        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("prop1"), ConfigMapValue::String(String::from("1")));
        expected_config_map.insert(String::from("prop2"), ConfigMapValue::String(String::from("2")));
        expected_config_map.insert(String::from("prop3"), ConfigMapValue::String(String::from("3")));
        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_say_config_file_with_error() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/dir/test.json", r#"{
            "prop2" 2
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.err().unwrap();
        assert_eq!(result.to_string(), concat!(
            "Error with 'https://dprint.dev/dir/test.json'. Error deserializing. ",
            "Expected a colon after the string or word in an object property on line 2 column 21."
        ));
    }

    #[tokio::test]
    async fn it_should_error_extending_locked_config() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {
                "prop": 5
            }
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.err().unwrap();
        assert_eq!(result.to_string(), concat!(
            "Error with 'https://dprint.dev/test.json'. ",
            "The configuration for \"test\" was locked, but a parent configuration specified it. ",
            "Locked configurations cannot have their properties overridden."
        ));
    }

    #[tokio::test]
    async fn it_should_get_locked_config() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/test.json"
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("6"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_locked_on_upstream_config() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "test": {
                "prop": 6,
                "other": "test"
            }
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {
                "locked": true,
                "prop": 7
            }
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("7"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_get_locked_config_and_not_care_if_no_properties_set_in_parent_config() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#.as_bytes());
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {}
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        let mut expected_config_map = HashMap::new();
        expected_config_map.insert(String::from("test"), ConfigMapValue::HashMap({
            let mut obj = HashMap::new();
            obj.insert(String::from("prop"), String::from("6"));
            obj.insert(String::from("other"), String::from("test"));
            obj
        }));

        assert_eq!(result.config_map, expected_config_map);
    }

    #[tokio::test]
    async fn it_should_handle_relative_remote_plugin() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["./test-plugin.wasm"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_remote_from_str("https://dprint.dev/test-plugin.wasm")
        ]);
    }

    #[tokio::test]
    async fn it_should_handle_relative_remote_plugin_in_extends() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "extends": "dir/test.json"
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/dir/test.json", r#"{
            "extends": "../otherDir/test.json"
        }"#.as_bytes());
        environment.add_remote_file("https://dprint.dev/otherDir/test.json", r#"{
            "plugins": [
                "../test/plugin.wasm",
            ]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_remote_from_str("https://dprint.dev/test/plugin.wasm")
        ]);
    }

    #[tokio::test]
    async fn it_should_handle_relative_local_plugins() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "plugins": ["./testing/asdf.wasm"],
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_local(PathBuf::from("/testing/asdf.wasm"))
        ]);
    }

    #[tokio::test]
    async fn it_should_handle_relative_local_plugins_in_extends() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "./other/test.json",
            "projectType": "openSource",
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/other/test.json"), r#"{
            "projectType": "openSource",
            "plugins": ["./testing/asdf.wasm"],
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.plugins, vec![
            PluginSourceReference::new_local(PathBuf::from("/other/testing/asdf.wasm"))
        ]);
    }

    #[tokio::test]
    async fn it_should_handle_incremental_flag_when_not_specified() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "plugins": ["./testing/asdf.wasm"],
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.incremental, false);
    }

    #[tokio::test]
    async fn it_should_handle_incremental_flag_when_true() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "incremental": true,
            "plugins": ["./testing/asdf.wasm"],
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.incremental, true);
    }

    #[tokio::test]
    async fn it_should_handle_incremental_flag_when_false() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "incremental": false,
            "plugins": ["./testing/asdf.wasm"],
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.incremental, false);
    }

    #[tokio::test]
    async fn it_should_ignore_non_wasm_plugins_in_remote_config() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["./test-plugin.plugin@checksum"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(result.plugins, vec![]);
        assert_eq!(environment.get_logged_messages(), vec![get_warn_non_wasm_plugins_message()]);
    }

    #[tokio::test]
    async fn it_should_ignore_non_wasm_plugins_in_remote_extends() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "https://dprint.dev/dir/test.json",
            "prop1": 1
        }"#).unwrap();
        environment.add_remote_file("https://dprint.dev/dir/test.json", r#"{
            "plugins": ["./test-plugin.plugin@checksum"]
        }"#.as_bytes());

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_non_wasm_plugins_message()]);
        assert_eq!(result.plugins, vec![]);
    }

    #[tokio::test]
    async fn it_should_not_allow_non_wasm_plugins_in_local_extends() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/dir/test.json"), r#"{
            "plugins": ["./test-plugin.plugin@checksum"]
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.plugins, vec![PluginSourceReference {
            path_source: PathSource::new_local(PathBuf::from("/dir/test-plugin.plugin")),
            checksum: Some(String::from("checksum")),
        }]);
    }
}
