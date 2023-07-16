use anyhow::bail;
use anyhow::Result;
use crossterm::style::Stylize;
use dprint_core::configuration::ConfigKeyValue;
use std::path::Path;
use thiserror::Error;

use crate::arg_parser::CliArgs;
use crate::configuration::deserialize_config;
use crate::configuration::ConfigMap;
use crate::configuration::ConfigMapValue;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::parse_plugin_source_reference;
use crate::plugins::PluginSourceReference;
use crate::utils::resolve_url_or_file_path;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::ResolvedPath;

use super::resolve_main_config_path::resolve_main_config_path;
use super::resolve_main_config_path::ResolvedConfigPath;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedConfig {
  pub resolved_path: ResolvedPath,
  /// The folder that should be considered the "root".
  pub base_path: CanonicalizedPathBuf,
  pub includes: Option<Vec<String>>,
  pub excludes: Option<Vec<String>>,
  pub plugins: Vec<PluginSourceReference>,
  pub incremental: Option<bool>,
  pub config_map: ConfigMap,
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum ResolveConfigError {
  #[error("No config file found at {}. Did you mean to create (dprint init) or specify one (--config <path>)?\n  Error: {inner:#}", .config_path.display())]
  NotFound {
    config_path: CanonicalizedPathBuf,
    inner: anyhow::Error,
  },
  Other(#[from] anyhow::Error),
}

pub fn resolve_config_from_args<TEnvironment: Environment>(args: &CliArgs, environment: &TEnvironment) -> Result<ResolvedConfig, ResolveConfigError> {
  let resolved_config_path = resolve_main_config_path(args, environment)?;
  let mut resolved_config = match resolve_config_from_path(&resolved_config_path, environment) {
    Ok(resolved_config) => resolved_config,
    Err(err) => {
      if !args.plugins.is_empty() && matches!(err, ResolveConfigError::NotFound { .. }) {
        // allow no config file when plugins are specified
        ResolvedConfig {
          config_map: ConfigMap::new(),
          base_path: resolved_config_path.base_path.clone(),
          resolved_path: resolved_config_path.resolved_path.clone(),
          excludes: None,
          includes: None,
          incremental: None,
          plugins: Vec::new(),
        }
      } else {
        return Err(err);
      }
    }
  };

  if !args.plugins.is_empty() {
    let base_path = PathSource::new_local(resolved_config_path.base_path);
    let mut plugins = Vec::with_capacity(args.plugins.len());
    for url_or_file_path in args.plugins.iter() {
      plugins.push(parse_plugin_source_reference(url_or_file_path, &base_path, environment)?);
    }

    resolved_config.plugins = plugins;
  }

  Ok(resolved_config)
}

pub fn resolve_config_from_path<TEnvironment: Environment>(
  resolved_config_path: &ResolvedConfigPath,
  environment: &TEnvironment,
) -> Result<ResolvedConfig, ResolveConfigError> {
  let base_source = resolved_config_path.resolved_path.source.parent();
  let config_file_path = &resolved_config_path.resolved_path.file_path;
  let config_map = get_config_map_from_path(config_file_path, environment)?;

  let mut config_map = match config_map {
    Ok(main_config_map) => main_config_map,
    Err(err) => {
      return Err(ResolveConfigError::NotFound {
        config_path: config_file_path.to_owned(),
        inner: err,
      });
    }
  };

  let plugins_vec = take_plugins_array_from_config_map(&mut config_map, &base_source, environment)?; // always take this out of the config map
  let plugins = filter_duplicate_plugin_sources({
    // filter out any non-wasm plugins from remote config
    if !resolved_config_path.resolved_path.is_local() {
      filter_non_wasm_plugins(plugins_vec, environment) // NEVER REMOVE THIS STATEMENT
    } else {
      plugins_vec
    }
  });

  // IMPORTANT
  // =========
  // Remove the includes from remote configuration since we don't want it
  // specifying something like system or some configuration
  // files that it could change. Basically, the end user should have 100%
  // control over what files get formatted.
  if !resolved_config_path.resolved_path.is_local() {
    // Careful! Don't be fancy and ensure this is removed.
    let removed_includes = config_map.remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && resolved_config_path.resolved_path.is_first_download {
      environment.log_stderr(&get_warn_includes_message());
    }
  }
  // =========

  let includes = take_array_from_config_map(&mut config_map, "includes")?;
  let excludes = take_array_from_config_map(&mut config_map, "excludes")?;
  let incremental = take_bool_from_config_map(&mut config_map, "incremental")?;
  config_map.remove("projectType"); // this was an old config property that's no longer used
  let extends = take_extends(&mut config_map)?;
  let mut resolved_config = ResolvedConfig {
    resolved_path: resolved_config_path.resolved_path.clone(),
    base_path: resolved_config_path.base_path.clone(),
    config_map,
    includes,
    excludes,
    plugins,
    incremental,
  };

  // resolve extends
  resolve_extends(&mut resolved_config, extends, &base_source, environment)?;

  Ok(resolved_config)
}

fn resolve_extends<TEnvironment: Environment>(
  resolved_config: &mut ResolvedConfig,
  extends: Vec<String>,
  base_path: &PathSource,
  environment: &TEnvironment,
) -> Result<()> {
  for url_or_file_path in extends {
    let resolved_path = resolve_url_or_file_path(&url_or_file_path, base_path, environment)?;
    match handle_config_file(&resolved_path, resolved_config, environment) {
      Ok(extends) => extends,
      Err(err) => bail!("Error with '{}'. {:#}", resolved_path.source.display(), err),
    }
  }
  Ok(())
}

fn handle_config_file<TEnvironment: Environment>(resolved_path: &ResolvedPath, resolved_config: &mut ResolvedConfig, environment: &TEnvironment) -> Result<()> {
  let config_file_path = &resolved_path.file_path;
  let mut new_config_map = match get_config_map_from_path(config_file_path, environment)? {
    Ok(config_map) => config_map,
    Err(err) => return Err(err),
  };
  let extends = take_extends(&mut new_config_map)?;

  // Discard any properties that shouldn't be inherited
  if !resolved_path.is_local() {
    // IMPORTANT
    // =========
    // Remove the includes from all referenced remote configuration since
    // we don't want it specifying something like system or some configuration
    // files that it could change. Basically, the end user should have 100%
    // control over what files get formatted.
    let removed_includes = new_config_map.remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && resolved_path.is_first_download {
      environment.log_stderr(&get_warn_includes_message());
    }
  }

  // combine excludes
  let excludes = take_array_from_config_map(&mut new_config_map, "excludes")?;
  if let Some(excludes) = excludes {
    match &mut resolved_config.excludes {
      Some(resolved_excludes) => resolved_excludes.extend(excludes),
      None => resolved_config.excludes = Some(excludes),
    }
  }

  // Also remove any non-wasm plugins, but only for remote configurations.
  // The assumption here is that the user won't be malicious to themselves.
  let plugins = take_plugins_array_from_config_map(&mut new_config_map, &resolved_path.source.parent(), environment)?;
  let plugins = if !resolved_path.is_local() {
    filter_non_wasm_plugins(plugins, environment)
  } else {
    plugins
  };
  // =========

  // combine plugins
  resolved_config.plugins.extend(plugins);

  for (key, value) in new_config_map {
    match value {
      ConfigMapValue::KeyValue(key_value) => {
        resolved_config.config_map.entry(key).or_insert(ConfigMapValue::KeyValue(key_value));
      }
      ConfigMapValue::Vec(items) => {
        resolved_config.config_map.entry(key).or_insert(ConfigMapValue::Vec(items));
      }
      ConfigMapValue::PluginConfig(obj) => {
        if let Some(resolved_config_obj) = resolved_config.config_map.get_mut(&key) {
          if let ConfigMapValue::PluginConfig(resolved_config_obj) = resolved_config_obj {
            // check for locked configuration
            if obj.locked && !resolved_config_obj.properties.is_empty() {
              bail!(
                concat!(
                  "The configuration for \"{}\" was locked, but a parent configuration specified it. ",
                  "Locked configurations cannot have their properties overridden."
                ),
                key
              );
            }

            // now the properties
            for (key, value) in obj.properties {
              resolved_config_obj.properties.entry(key).or_insert(value);
            }

            // Set the associations if they aren't overwritten in the parent
            // config. This is ok to do because process plugins and includes/excludes
            // aren't inherited from other config.
            if resolved_config_obj.associations.is_none() {
              resolved_config_obj.associations = obj.associations;
            }
          }
        } else {
          resolved_config.config_map.insert(key, ConfigMapValue::PluginConfig(obj));
        }
      }
    }
  }

  resolve_extends(resolved_config, extends, &resolved_path.source.parent(), environment)?;

  Ok(())
}

fn take_extends(config_map: &mut ConfigMap) -> Result<Vec<String>> {
  match config_map.remove("extends") {
    Some(ConfigMapValue::KeyValue(ConfigKeyValue::String(url_or_file_path))) => Ok(vec![url_or_file_path]),
    Some(ConfigMapValue::Vec(url_or_file_paths)) => Ok(url_or_file_paths),
    Some(_) => bail!("Extends in configuration must be a string or an array of strings."),
    None => Ok(Vec::new()),
  }
}

fn get_config_map_from_path(file_path: impl AsRef<Path>, environment: &impl Environment) -> Result<Result<ConfigMap>> {
  let config_file_text = match environment.read_file(file_path) {
    Ok(file_text) => file_text,
    Err(err) => return Ok(Err(err)),
  };

  let result = match deserialize_config(&config_file_text) {
    Ok(map) => map,
    Err(e) => bail!("Error deserializing. {}", e.to_string()),
  };

  Ok(Ok(result))
}

fn take_plugins_array_from_config_map(
  config_map: &mut ConfigMap,
  base_path: &PathSource,
  environment: &impl Environment,
) -> Result<Vec<PluginSourceReference>> {
  let plugin_url_or_file_paths = take_array_from_config_map(config_map, "plugins")?.unwrap_or_default();
  let mut plugins = Vec::with_capacity(plugin_url_or_file_paths.len());
  for url_or_file_path in plugin_url_or_file_paths {
    plugins.push(parse_plugin_source_reference(&url_or_file_path, base_path, environment)?);
  }
  Ok(plugins)
}

fn take_array_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Option<Vec<String>>> {
  match config_map.remove(property_name) {
    Some(ConfigMapValue::Vec(elements)) => Ok(Some(elements)),
    Some(_) => bail!("Expected array in '{}' property.", property_name),
    None => Ok(None),
  }
}

fn take_bool_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Option<bool>> {
  if let Some(value) = config_map.remove(property_name) {
    match value {
      ConfigMapValue::KeyValue(ConfigKeyValue::Bool(value)) => Ok(Some(value)),
      _ => bail!("Expected boolean in '{}' property.", property_name),
    }
  } else {
    Ok(None)
  }
}

fn filter_non_wasm_plugins(plugins: Vec<PluginSourceReference>, environment: &impl Environment) -> Vec<PluginSourceReference> {
  if plugins.iter().any(|plugin| plugin.plugin_kind() != Some(PluginKind::Wasm)) {
    environment.log_stderr(&get_warn_non_wasm_plugins_message());
    plugins.into_iter().filter(|plugin| plugin.plugin_kind() == Some(PluginKind::Wasm)).collect()
  } else {
    plugins
  }
}

fn get_warn_includes_message() -> String {
  format!(
    "{} The 'includes' property is ignored for security reasons on remote configuration.",
    "Note: ".bold(),
  )
}

fn get_warn_non_wasm_plugins_message() -> String {
  format!("{} Non-wasm plugins are ignored for security reasons on remote configuration.", "Note: ".bold(),)
}

fn filter_duplicate_plugin_sources(plugin_sources: Vec<PluginSourceReference>) -> Vec<PluginSourceReference> {
  let mut path_source_set = std::collections::HashSet::new();

  plugin_sources
    .into_iter()
    .filter(|source| path_source_set.insert(source.path_source.clone()))
    .collect()
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use crate::arg_parser::parse_args;
  use crate::configuration::RawPluginConfig;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::utils::TestStdInReader;
  use anyhow::Result;
  use dprint_core::configuration::ConfigKeyMap;
  use pretty_assertions::assert_eq;

  use super::*;

  fn get_result(url: &str, environment: &impl Environment) -> Result<ResolvedConfig, ResolveConfigError> {
    let args = parse_args(
      vec![String::from(""), String::from("check"), String::from("-c"), String::from(url)],
      TestStdInReader::default(),
    )
    .unwrap();
    resolve_config_from_args(&args, environment)
  }

  #[test]
  fn should_get_local_config_file() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": ["test"],
            "excludes": ["test-excludes"]
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.base_path, CanonicalizedPathBuf::new_for_testing("/"));
    assert_eq!(result.resolved_path.is_local(), true);
    assert_eq!(result.config_map.contains_key("includes"), false);
    assert_eq!(result.config_map.contains_key("excludes"), false);
    assert_eq!(result.includes, Some(vec!["test".to_string()]));
    assert_eq!(result.excludes, Some(vec!["test-excludes".to_string()]));
  }

  #[test]
  fn should_get_remote_config_file() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.base_path, CanonicalizedPathBuf::new_for_testing("/"));
    assert_eq!(result.resolved_path.is_remote(), true);
  }

  #[test]
  fn should_warn_on_first_download_for_remote_config_with_includes() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": ["test"]
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(environment.take_stderr_messages(), vec![get_warn_includes_message()]);
    assert_eq!(result.includes, None);

    environment.clear_logs();
    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stderr_messages().len(), 0); // no warning this time
    assert_eq!(result.includes, None);
  }

  #[test]
  fn should_warn_on_first_download_for_remote_config_with_includes_and_excludes() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": [],
            "excludes": []
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stderr_messages(), vec![get_warn_includes_message()]);
    assert_eq!(result.includes, None);
    assert_eq!(result.excludes, Some(vec![]));

    environment.clear_logs();
    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stderr_messages().len(), 0); // no warning this time
    assert_eq!(result.includes, None);
    assert_eq!(result.excludes, Some(vec![]));
  }

  #[test]
  fn should_not_warn_remove_config_no_includes_or_excludes() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#
        .as_bytes(),
    );

    get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
  }

  #[test]
  fn should_handle_single_extends() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
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
            "excludes": ["test-excludes"]
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "otherProp": 6,
            "test": {
                "prop": 5
            },
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.base_path, CanonicalizedPathBuf::new_for_testing("/"));
    assert_eq!(result.resolved_path.is_local(), true);
    assert_eq!(result.includes, None);
    assert_eq!(result.excludes, Some(vec!["test-excludes".to_string()]));
    assert_eq!(
      result.plugins,
      vec![
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
      ]
    );

    let expected_config_map = ConfigMap::from([
      (String::from("lineWidth"), ConfigMapValue::from_i32(1)),
      (String::from("otherProp"), ConfigMapValue::from_i32(6)),
      (String::from("otherProp2"), ConfigMapValue::from_str("a")),
      (
        String::from("test"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([
            (String::from("prop"), ConfigKeyValue::from_i32(5)),
            (String::from("other"), ConfigKeyValue::from_str("test")),
          ]),
        }),
      ),
      (
        String::from("test2"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([(String::from("prop"), ConfigKeyValue::from_i32(2))]),
        }),
      ),
    ]);

    assert_eq!(result.config_map, expected_config_map);
    let logged_warnings = environment.take_stderr_messages();
    assert_eq!(logged_warnings, vec![get_warn_includes_message()]);
  }

  #[test]
  fn should_handle_array_extends() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
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
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/test2.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin3.wasm"],
            "otherProp": 7,
            "asdf": 4,
            "test": {
                "other": "test2"
            }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": [
                "https://dprint.dev/test.json",
                "https://dprint.dev/test2.json",
            ],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "test": {
                "prop": 5
            },
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.includes, None);
    assert_eq!(result.excludes, None);
    assert_eq!(
      result.plugins,
      vec![
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin3.wasm"),
      ]
    );

    let expected_config_map = ConfigMap::from([
      (String::from("lineWidth"), ConfigMapValue::from_i32(1)),
      (String::from("otherProp"), ConfigMapValue::from_i32(6)),
      (String::from("asdf"), ConfigMapValue::from_i32(4)),
      (
        String::from("test"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([
            (String::from("prop"), ConfigKeyValue::from_i32(5)),
            (String::from("other"), ConfigKeyValue::from_str("test")),
          ]),
        }),
      ),
      (
        String::from("test2"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([(String::from("prop"), ConfigKeyValue::from_i32(2))]),
        }),
      ),
    ]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_extends_within_an_extends() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
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
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/test2.json",
      r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin3.wasm"],
            "otherProp": 7,
            "asdf": 4,
            "test": {
                "other": "test2"
            }
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/test3.json",
      r#"{
            "asdf": 4,
            "newProp": "test"
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": [
                "https://dprint.dev/test.json"
                "https://dprint.dev/test3.json" // should have lowest precedence
            ],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "lineWidth": 1,
            "test": {
                "prop": 5
            },
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.includes, None);
    assert_eq!(result.excludes, None);
    assert_eq!(
      result.plugins,
      vec![
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin2.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin3.wasm"),
      ]
    );

    let expected_config_map = ConfigMap::from([
      (String::from("lineWidth"), ConfigMapValue::from_i32(1)),
      (String::from("otherProp"), ConfigMapValue::from_i32(6)),
      (String::from("asdf"), ConfigMapValue::from_i32(4)),
      (String::from("newProp"), ConfigMapValue::from_str("test")),
      (
        String::from("test"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([
            (String::from("prop"), ConfigKeyValue::from_i32(5)),
            (String::from("other"), ConfigKeyValue::from_str("test")),
          ]),
        }),
      ),
      (
        String::from("test2"),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          properties: ConfigKeyMap::from([(String::from("prop"), ConfigKeyValue::from_i32(2))]),
        }),
      ),
    ]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_relative_remote_extends() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/dir/test.json",
      r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/otherDir/test.json",
      r#"{
            "extends": "https://test.dprint.dev/test.json",
            "prop3": 3
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://test.dprint.dev/test.json",
      r#"{
            "extends": [
                "other.json",
                "dir/test.json"
            ],
            "prop4": 4,
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://test.dprint.dev/other.json",
      r#"{
            "prop5": 5,
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://test.dprint.dev/dir/test.json",
      r#"{
            "prop6": 6,
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);

    let expected_config_map = ConfigMap::from([
      (String::from("prop1"), ConfigMapValue::from_i32(1)),
      (String::from("prop2"), ConfigMapValue::from_i32(2)),
      (String::from("prop3"), ConfigMapValue::from_i32(3)),
      (String::from("prop4"), ConfigMapValue::from_i32(4)),
      (String::from("prop5"), ConfigMapValue::from_i32(5)),
      (String::from("prop6"), ConfigMapValue::from_i32(6)),
    ]);
    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_remote_in_local_extends() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/dir/test.json",
            "prop1": 1
        }"#,
      )
      .unwrap();
    environment.add_remote_file(
      "https://dprint.dev/dir/test.json",
      r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/otherDir/test.json",
      r#"{
            "prop3": 3
        }"#
        .as_bytes(),
    );

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);

    let expected_config_map = ConfigMap::from([
      (String::from("prop1"), ConfigMapValue::from_i32(1)),
      (String::from("prop2"), ConfigMapValue::from_i32(2)),
      (String::from("prop3"), ConfigMapValue::from_i32(3)),
    ]);
    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_relative_local_extends() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#,
      )
      .unwrap();
    environment
      .write_file(
        &PathBuf::from("/dir/test.json"),
        r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#,
      )
      .unwrap();
    environment
      .write_file(
        &PathBuf::from("/otherDir/test.json"),
        r#"{
            "prop3": 3
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);

    let expected_config_map = ConfigMap::from([
      (String::from("prop1"), ConfigMapValue::from_i32(1)),
      (String::from("prop2"), ConfigMapValue::from_i32(2)),
      (String::from("prop3"), ConfigMapValue::from_i32(3)),
    ]);
    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_say_config_file_with_error() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/dir/test.json",
      r#"{
            "prop2" 2
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).err().unwrap();
    assert_eq!(
      result.to_string(),
      concat!(
        "Error with 'https://dprint.dev/dir/test.json'. Error deserializing. ",
        "Expected a colon after the string or word in an object property on line 2 column 21."
      )
    );
  }

  #[test]
  fn should_error_extending_locked_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {
                "prop": 5
            }
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).err().unwrap();
    assert_eq!(
      result.to_string(),
      concat!(
        "Error with 'https://dprint.dev/test.json'. ",
        "The configuration for \"test\" was locked, but a parent configuration specified it. ",
        "Locked configurations cannot have their properties overridden."
      )
    );
  }

  #[test]
  fn should_get_locked_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json"
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    let expected_config_map = ConfigMap::from([(
      String::from("test"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: true,
        associations: None,
        properties: ConfigKeyMap::from([
          (String::from("prop"), ConfigKeyValue::from_i32(6)),
          (String::from("other"), ConfigKeyValue::from_str("test")),
        ]),
      }),
    )]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_locked_on_upstream_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "test": {
                "prop": 6,
                "other": "test"
            }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {
                "locked": true,
                "prop": 7
            }
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    let expected_config_map = ConfigMap::from([(
      String::from("test"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: true,
        associations: None,
        properties: ConfigKeyMap::from([
          (String::from("prop"), ConfigKeyValue::from_i32(7)),
          (String::from("other"), ConfigKeyValue::from_str("test")),
        ]),
      }),
    )]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_get_locked_config_and_not_care_if_no_properties_set_in_parent_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "test": {
                "locked": true,
                "prop": 6,
                "other": "test"
            }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {}
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    let expected_config_map = ConfigMap::from([(
      String::from("test"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: false,
        associations: None,
        properties: ConfigKeyMap::from([
          (String::from("prop"), ConfigKeyValue::from_i32(6)),
          (String::from("other"), ConfigKeyValue::from_str("test")),
        ]),
      }),
    )]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_use_associations_on_extended_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
          "test": {
            "associations": "test"
          }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {}
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    let expected_config_map = ConfigMap::from([(
      String::from("test"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: false,
        associations: Some(vec!["test".to_string()]),
        properties: ConfigKeyMap::new(),
      }),
    )]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_override_associations_on_extended_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
          "test": {
            "associations": "test"
          }
        }"#
        .as_bytes(),
    );
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/test.json",
            "test": {
              "associations": ["test1", "test2"]
            }
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    let expected_config_map = ConfigMap::from([(
      String::from("test"),
      ConfigMapValue::PluginConfig(RawPluginConfig {
        locked: false,
        associations: Some(vec!["test1".to_string(), "test2".to_string()]),
        properties: ConfigKeyMap::new(),
      }),
    )]);

    assert_eq!(result.config_map, expected_config_map);
  }

  #[test]
  fn should_handle_relative_remote_plugin() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["./test-plugin.wasm"]
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(
      result.plugins,
      vec![PluginSourceReference::new_remote_from_str("https://dprint.dev/test-plugin.wasm")]
    );
  }

  #[test]
  fn should_handle_relative_remote_plugin_in_extends() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "extends": "dir/test.json"
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/dir/test.json",
      r#"{
            "extends": "../otherDir/test.json"
        }"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/otherDir/test.json",
      r#"{
            "plugins": [
                "../test/plugin.wasm",
            ]
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(
      result.plugins,
      vec![PluginSourceReference::new_remote_from_str("https://dprint.dev/test/plugin.wasm")]
    );
  }

  #[test]
  fn should_handle_relative_local_plugins() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.plugins, vec![PluginSourceReference::new_local("/testing/asdf.wasm")]);
  }

  #[test]
  fn should_handle_relative_local_plugins_in_extends() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "./other/test.json",
        }"#,
      )
      .unwrap();
    environment
      .write_file(
        &PathBuf::from("/other/test.json"),
        r#"{
            "projectType": "openSource", // test having this in base config
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.plugins, vec![PluginSourceReference::new_local("/other/testing/asdf.wasm")]);
  }

  #[test]
  fn should_handle_incremental_flag_when_not_specified() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.incremental, None);
  }

  #[test]
  fn should_handle_incremental_flag_when_true() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "incremental": true,
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.incremental, Some(true));
  }

  #[test]
  fn should_handle_incremental_flag_when_false() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "incremental": false,
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.incremental, Some(false));
  }

  #[test]
  fn should_ignore_non_wasm_plugins_in_remote_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
            "plugins": ["./test-plugin.json@checksum"]
        }"#
        .as_bytes(),
    );

    let result = get_result("https://dprint.dev/test.json", &environment).unwrap();
    assert_eq!(result.plugins, vec![]);
    assert_eq!(environment.take_stderr_messages(), vec![get_warn_non_wasm_plugins_message()]);
  }

  #[test]
  fn should_ignore_non_wasm_plugins_in_remote_extends() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "https://dprint.dev/dir/test.json",
            "prop1": 1
        }"#,
      )
      .unwrap();
    environment.add_remote_file(
      "https://dprint.dev/dir/test.json",
      r#"{
            "plugins": ["./test-plugin.json@checksum"]
        }"#
        .as_bytes(),
    );

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stderr_messages(), vec![get_warn_non_wasm_plugins_message()]);
    assert_eq!(result.plugins, vec![]);
  }

  #[test]
  fn should_not_allow_non_wasm_plugins_in_local_extends() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#,
      )
      .unwrap();
    environment
      .write_file(
        &PathBuf::from("/dir/test.json"),
        r#"{
            "plugins": ["./test-plugin.json@checksum"]
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(
      result.plugins,
      vec![PluginSourceReference {
        path_source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/dir/test-plugin.json")),
        checksum: Some(String::from("checksum")),
      }]
    );
  }

  #[test]
  fn should_ignore_project_type() {
    // ignore the projectType property
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": ["test"],
            "excludes": ["test"]
        }"#,
      )
      .unwrap();

    let result = get_result("/test.json", &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
    assert_eq!(result.config_map.is_empty(), true); // should not include projectType
  }
}
