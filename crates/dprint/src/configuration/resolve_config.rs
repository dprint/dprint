use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
use anyhow::bail;
use crossterm::style::Stylize;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::configuration::ConfigKeyValue;
use thiserror::Error;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::SubCommand;
use crate::configuration::ConfigMap;
use crate::configuration::ConfigMapValue;
use crate::configuration::deserialize_config;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::plugins::parse_plugin_source_reference;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::ResolvedPath;
use crate::utils::ShowConfirmStrategy;
use crate::utils::resolve_url_or_file_path;

use super::resolve_main_config_path::ResolvedConfigPath;
use super::resolve_main_config_path::resolve_main_config_path;

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
  #[error(
    "No config file found at {}. Did you mean to create (dprint init) or specify one (--config <path>)?",
    .config_path.display(),
  )]
  NotFound {
    config_path: CanonicalizedPathBuf,
    #[source]
    inner: Option<anyhow::Error>,
  },
  #[error("Config discovery was disabled and no plugins (--plugins <url/path>) and/or config (--config <path>) was specified.")]
  ConfigDiscoveryDisabled,
  Other(#[from] anyhow::Error),
}

pub async fn resolve_config_from_args(args: &CliArgs, environment: &impl Environment) -> Result<ResolvedConfig, ResolveConfigError> {
  struct ConfirmFormatGlobalConfigStrategy<'a> {
    directory: &'a Path,
  }

  impl ShowConfirmStrategy for ConfirmFormatGlobalConfigStrategy<'_> {
    fn render(&self, selected: Option<bool>) -> String {
      format!(
        "{} You're not in a dprint project. Format '{}' anyway? {}{}",
        "Warning".yellow(),
        self.directory.display(),
        match selected {
          Some(true) => "Y",
          Some(false) => "N",
          None => "(Y/n) \u{2588}",
        },
        match selected {
          Some(_) => "".stylize(),
          None => "\n\nHint: Specify the directory to bypass this prompt in the future (ex. `dprint fmt .`)".grey(),
        },
      )
    }

    fn default_value(&self) -> bool {
      true
    }
  }

  let resolved_config_path = resolve_main_config_path(args, environment).await?;
  let mut resolved_config = match resolved_config_path {
    Some(resolved_config_path) => {
      if resolved_config_path.is_global_config
        && let SubCommand::Fmt(fmt) = &args.sub_command
        && !fmt.allow_no_files
        && fmt.patterns.include_patterns.is_empty()
        && fmt.patterns.include_pattern_overrides.is_none()
        && !(args.config_discovery_arg_set() && matches!(args.config_discovery(environment), ConfigDiscovery::Global))
      {
        if !environment.is_terminal_interactive() {
          return Err(ResolveConfigError::Other(anyhow::anyhow!(
            "Did not format directory without configuration file. Run `dprint fmt .` or `dprint fmt --config-discovery=global` to bypass this error."
          )));
        } else if !environment.confirm_with_strategy(&ConfirmFormatGlobalConfigStrategy {
          directory: resolved_config_path.base_path.as_ref(),
        })? {
          return Err(ResolveConfigError::Other(anyhow::anyhow!("Confirmation cancelled.")));
        }
      }
      resolve_config_from_path(&resolved_config_path, environment).await?
    }
    None => {
      if !args.plugins.is_empty() {
        // allow no config file when plugins are specified
        ResolvedConfig {
          config_map: ConfigMap::new(),
          base_path: environment.cwd().clone(),
          resolved_path: ResolvedPath::local(environment.cwd().join_panic_relative("dprint.json")),
          excludes: None,
          includes: None,
          incremental: None,
          plugins: Vec::new(),
        }
      } else if args.config_discovery(environment).traverse_ancestors() {
        return Err(ResolveConfigError::NotFound {
          config_path: environment.cwd().join_panic_relative("dprint.json"),
          inner: None,
        });
      } else {
        return Err(ResolveConfigError::ConfigDiscoveryDisabled);
      }
    }
  };

  if !args.plugins.is_empty() {
    let base_path = PathSource::new_local(environment.cwd());
    let mut plugins = Vec::with_capacity(args.plugins.len());
    for url_or_file_path in args.plugins.iter() {
      plugins.push(parse_plugin_source_reference(url_or_file_path, &base_path, environment)?);
    }

    resolved_config.plugins = plugins;
  }

  Ok(resolved_config)
}

pub async fn resolve_config_from_path<TEnvironment: Environment>(
  resolved_config_path: &ResolvedConfigPath,
  environment: &TEnvironment,
) -> Result<ResolvedConfig, ResolveConfigError> {
  let base_source = resolved_config_path.resolved_path.source.parent();
  let config_file_path = &resolved_config_path.resolved_path.file_path;
  let config_map = get_config_map_from_path(
    ConfigPathContext {
      current: &resolved_config_path.resolved_path,
      origin: &resolved_config_path.resolved_path,
    },
    environment,
  )
  .map_err(|err| anyhow::anyhow!("{:#}\n    at {}", err, resolved_config_path.resolved_path.source.display()))?;

  let mut config_map = match config_map {
    Ok(main_config_map) => main_config_map,
    Err(err) => {
      return Err(ResolveConfigError::NotFound {
        config_path: config_file_path.to_owned(),
        inner: Some(err),
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
    let removed_includes = config_map.shift_remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && resolved_config_path.resolved_path.is_first_download {
      log_warn!(environment, &get_warn_includes_message());
    }
  }
  // =========

  let includes = take_array_from_config_map(&mut config_map, "includes")?;
  let excludes = take_array_from_config_map(&mut config_map, "excludes")?;

  let incremental = take_bool_from_config_map(&mut config_map, "incremental")?;
  config_map.shift_remove("projectType"); // this was an old config property that's no longer used
  let extends = take_extends(&mut config_map)?;
  let resolved_config = ResolvedConfig {
    resolved_path: resolved_config_path.resolved_path.clone(),
    base_path: resolved_config_path.base_path.clone(),
    config_map,
    includes,
    excludes,
    plugins,
    incremental,
  };

  // resolve extends
  Ok(resolve_extends(resolved_config, extends, base_source, environment.clone()).await?)
}

fn resolve_extends<TEnvironment: Environment>(
  mut resolved_config: ResolvedConfig,
  extends: Vec<String>,
  base_path: PathSource,
  environment: TEnvironment,
) -> LocalBoxFuture<'static, Result<ResolvedConfig>> {
  // boxed because of recursion
  async move {
    for url_or_file_path in extends {
      let resolved_path = resolve_url_or_file_path(&url_or_file_path, &base_path, &environment).await?;
      resolved_config = match handle_config_file(&resolved_path, resolved_config, &environment).await {
        Ok(resolved_config) => resolved_config,
        Err(err) => bail!("{:#}\n    at {}", err, resolved_path.source.display()),
      }
    }
    Ok(resolved_config)
  }
  .boxed_local()
}

async fn handle_config_file<TEnvironment: Environment>(
  resolved_path: &ResolvedPath,
  mut resolved_config: ResolvedConfig,
  environment: &TEnvironment,
) -> Result<ResolvedConfig> {
  let mut new_config_map = match get_config_map_from_path(
    ConfigPathContext {
      current: resolved_path,
      origin: &resolved_config.resolved_path,
    },
    environment,
  )? {
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
    let removed_includes = new_config_map.shift_remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && resolved_path.is_first_download {
      log_warn!(environment, &get_warn_includes_message());
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

  resolve_extends(resolved_config, extends, resolved_path.source.parent(), environment.clone()).await
}

fn take_extends(config_map: &mut ConfigMap) -> Result<Vec<String>> {
  match config_map.shift_remove("extends") {
    Some(ConfigMapValue::KeyValue(ConfigKeyValue::String(url_or_file_path))) => Ok(vec![url_or_file_path]),
    Some(ConfigMapValue::Vec(url_or_file_paths)) => Ok(url_or_file_paths),
    Some(_) => bail!("Extends in configuration must be a string or an array of strings."),
    None => Ok(Vec::new()),
  }
}

#[derive(Debug, Clone, Copy)]
struct ConfigPathContext<'a> {
  /// The path of the configuration file being resolved.
  ///
  /// This could be a config being extended.
  current: &'a ResolvedPath,
  /// The original configuration file that may have extended
  /// the current configuration file.
  origin: &'a ResolvedPath,
}

fn get_config_map_from_path(path: ConfigPathContext, environment: &impl Environment) -> Result<Result<ConfigMap>> {
  let config_file_text = match environment.read_file(&path.current.file_path) {
    Ok(file_text) => file_text,
    Err(err) => return Ok(Err(err)),
  };

  let mut result = match deserialize_config(&config_file_text) {
    Ok(map) => map,
    Err(e) => bail!("Error deserializing. {}", e.to_string()),
  };
  template_expand(path, &mut result)?;

  Ok(Ok(result))
}

fn template_expand(path_ctx: ConfigPathContext, config_map: &mut ConfigMap) -> Result<()> {
  fn handle_string(path: ConfigPathContext, value: &mut String) -> Result<()> {
    let mut parts = Vec::with_capacity(16); // unlikely to be more than this
    let mut last_index = 0;
    let mut chars = value.char_indices().peekable();

    while let Some((index, c)) = chars.next() {
      if c == '\\' && matches!(chars.peek(), Some((_, '$'))) {
        parts.push(Cow::Borrowed(&value[last_index..index]));
        last_index = index + 1; // skip '\'
        chars.next(); // skip '$'
      } else if c == '$' && matches!(chars.peek(), Some((_, '{'))) {
        // Found start of template literal ${...}
        chars.next(); // skip '{'

        let mut template_name = "";
        let template_start_index = index + 2; // skip '{' and '$'
        for (current_index, inner_char) in chars.by_ref() {
          if inner_char == '}' {
            template_name = &value[template_start_index..current_index];
            parts.push(Cow::Borrowed(&value[last_index..index]));
            last_index = current_index + 1; // skip '}'
            break;
          }
        }

        match template_name {
          "configDir" => {
            if path.current.is_remote() {
              bail!("Cannot use ${{configDir}} template in remote configuration files. Maybe use ${{originConfigDir}} instead?");
            }
            parts.push(Cow::Owned(path.current.file_path.parent().unwrap().to_string_lossy().to_string()));
          }
          "originConfigDir" => {
            if path.origin.is_remote() {
              bail!(
                "Cannot use ${{originConfigDir}} template when the origin configuration file ({}) is remote.",
                path.origin.source.display(),
              );
            }
            parts.push(Cow::Owned(path.origin.file_path.parent().unwrap().to_string_lossy().to_string()));
          }
          "" => {
            // ignore
          }
          _ => {
            bail!(
              concat!(
                "Unknown template literal ${{{}}}. Only ${{configDir}} and ${{originConfigDir}} are supported. ",
                "If you meant to pass this to a plugin, escape the dollar sign with two back slashes.",
              ),
              template_name,
            );
          }
        }
      }
    }

    if !parts.is_empty() {
      parts.push(Cow::Borrowed(&value[last_index..]));
      *value = parts.join("");
    }

    Ok(())
  }

  fn handle_config_key_value(path_ctx: ConfigPathContext, value: &mut ConfigKeyValue) -> Result<()> {
    match value {
      ConfigKeyValue::String(value) => {
        handle_string(path_ctx, value)?;
      }
      ConfigKeyValue::Array(array) => {
        for value in array {
          handle_config_key_value(path_ctx, value)?;
        }
      }
      ConfigKeyValue::Object(obj) => {
        for value in obj.values_mut() {
          handle_config_key_value(path_ctx, value)?;
        }
      }
      ConfigKeyValue::Number(_) | ConfigKeyValue::Bool(_) | ConfigKeyValue::Null => {
        // ignore
      }
    }
    Ok(())
  }

  for value in config_map.values_mut() {
    match value {
      ConfigMapValue::KeyValue(kv) => {
        handle_config_key_value(path_ctx, kv)?;
      }
      ConfigMapValue::PluginConfig(config) => {
        for value in config.properties.values_mut() {
          handle_config_key_value(path_ctx, value)?;
        }
      }
      ConfigMapValue::Vec(vec) => {
        for value in vec {
          handle_string(path_ctx, value)?;
        }
      }
    }
  }

  Ok(())
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
  match config_map.shift_remove(property_name) {
    Some(ConfigMapValue::Vec(elements)) => Ok(Some(elements)),
    Some(_) => bail!("Expected array in '{}' property.", property_name),
    None => Ok(None),
  }
}

fn take_bool_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Option<bool>> {
  if let Some(value) = config_map.shift_remove(property_name) {
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
    log_warn!(environment, &get_warn_non_wasm_plugins_message());
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

  async fn get_result(url: &str, environment: &impl Environment) -> Result<ResolvedConfig, ResolveConfigError> {
    let args = parse_args(
      vec![String::from(""), String::from("check"), String::from("-c"), String::from(url)],
      TestStdInReader::default(),
    )
    .unwrap();
    resolve_config_from_args(&args, environment).await
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.base_path, CanonicalizedPathBuf::new_for_testing("/"));
      assert_eq!(result.resolved_path.is_local(), true);
      assert_eq!(result.config_map.contains_key("includes"), false);
      assert_eq!(result.config_map.contains_key("excludes"), false);
      assert_eq!(result.includes, Some(vec!["test".to_string()]));
      assert_eq!(result.excludes, Some(vec!["test-excludes".to_string()]));
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.base_path, CanonicalizedPathBuf::new_for_testing("/"));
      assert_eq!(result.resolved_path.is_remote(), true);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(environment.take_stderr_messages(), vec![get_warn_includes_message()]);
      assert_eq!(result.includes, None);

      environment.clear_logs();
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stderr_messages().len(), 0); // no warning this time
      assert_eq!(result.includes, None);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stderr_messages(), vec![get_warn_includes_message()]);
      assert_eq!(result.includes, None);
      assert_eq!(result.excludes, Some(vec![]));

      environment.clear_logs();
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stderr_messages().len(), 0); // no warning this time
      assert_eq!(result.includes, None);
      assert_eq!(result.excludes, Some(vec![]));
    });
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

    environment.clone().run_in_runtime(async move {
      get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);

      let expected_config_map = ConfigMap::from([
        (String::from("prop1"), ConfigMapValue::from_i32(1)),
        (String::from("prop2"), ConfigMapValue::from_i32(2)),
        (String::from("prop3"), ConfigMapValue::from_i32(3)),
      ]);
      assert_eq!(result.config_map, expected_config_map);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);

      let expected_config_map = ConfigMap::from([
        (String::from("prop1"), ConfigMapValue::from_i32(1)),
        (String::from("prop2"), ConfigMapValue::from_i32(2)),
        (String::from("prop3"), ConfigMapValue::from_i32(3)),
      ]);
      assert_eq!(result.config_map, expected_config_map);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.err().unwrap();
      assert_eq!(
        result.to_string(),
        concat!(
          "Error deserializing. Expected colon after the string or word in object property on line 2 column 21\n",
          "    at https://dprint.dev/dir/test.json"
        )
      );
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.err().unwrap();
      assert_eq!(
        result.to_string(),
        concat!(
          "The configuration for \"test\" was locked, but a parent configuration specified it. ",
          "Locked configurations cannot have their properties overridden.\n",
          "    at https://dprint.dev/test.json",
        )
      );
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
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
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference::new_remote_from_str("https://dprint.dev/test-plugin.wasm")]
      );
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference::new_remote_from_str("https://dprint.dev/test/plugin.wasm")]
      );
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.plugins, vec![PluginSourceReference::new_local("/testing/asdf.wasm")]);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.plugins, vec![PluginSourceReference::new_local("/other/testing/asdf.wasm")]);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.incremental, None);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.incremental, Some(true));
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.incremental, Some(false));
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
      assert_eq!(result.plugins, vec![]);
      assert_eq!(environment.take_stderr_messages(), vec![get_warn_non_wasm_plugins_message()]);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stderr_messages(), vec![get_warn_non_wasm_plugins_message()]);
      assert_eq!(result.plugins, vec![]);
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference {
          path_source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/dir/test-plugin.json")),
          checksum: Some(String::from("checksum")),
        }]
      );
    });
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

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.config_map.is_empty(), true); // should not include projectType
    });
  }

  #[test]
  fn should_resolve_config_dir_local_file() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
      "extends": "./next.json",
      "otherPlugin": {
        "value": "${originConfigDir}/origin"
      }
}"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/next.json",
      r#"{
      "final": {
        "value": "${originConfigDir}/final && \\${configDir}/escaped"
      }
}"#
        .as_bytes(),
    );
    environment
      .write_file(
        "/dir/dprint.json",
        r#"{
      "extends": "https://dprint.dev/test.json",
      "plugin": {
        "value": "${configDir}/test && ${originConfigDir}/other"
      }
}"#,
      )
      .unwrap();

    environment.clone().run_in_runtime(async move {
      let config = get_result("/dir/dprint.json", &environment).await.unwrap();
      assert_eq!(
        config.config_map,
        ConfigMap::from([
          (
            "plugin".to_string(),
            ConfigMapValue::PluginConfig(RawPluginConfig {
              locked: false,
              associations: None,
              properties: ConfigKeyMap::from([(String::from("value"), ConfigKeyValue::from_str("/dir/test && /dir/other"))]),
            }),
          ),
          (
            "otherPlugin".to_string(),
            ConfigMapValue::PluginConfig(RawPluginConfig {
              locked: false,
              associations: None,
              properties: ConfigKeyMap::from([(String::from("value"), ConfigKeyValue::from_str("/dir/origin"))]),
            }),
          ),
          (
            "final".to_string(),
            ConfigMapValue::PluginConfig(RawPluginConfig {
              locked: false,
              associations: None,
              properties: ConfigKeyMap::from([(String::from("value"), ConfigKeyValue::from_str("/dir/final && ${configDir}/escaped"))]),
            }),
          )
        ])
      );
    });
  }

  #[test]
  fn should_error_remote_config_file_with_config_dir() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
      "plugin": {
        "value": "${configDir}/test"
      }
}"#
        .as_bytes(),
    );

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.err().unwrap();
      assert_eq!(
        result.to_string(),
        "Cannot use ${configDir} template in remote configuration files. Maybe use ${originConfigDir} instead?\n    at https://dprint.dev/test.json"
      );
    });
  }

  #[test]
  fn should_error_remote_origin_config_file_with_config_dir() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
      "extends": "./next.json",
      "otherPlugin": {
        "value": "test"
      }
}"#
        .as_bytes(),
    );
    environment.add_remote_file(
      "https://dprint.dev/next.json",
      r#"{
      "final": {
        "value": "${originConfigDir}/final"
      }
}"#
        .as_bytes(),
    );

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.err().unwrap();
      assert_eq!(
        result.to_string(),
        "Cannot use ${originConfigDir} template when the origin configuration file (https://dprint.dev/test.json) is remote.\n    at https://dprint.dev/next.json"
      );
    });
  }

  #[test]
  fn should_error_unknown_template() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
      "plugin": {
        "value": "${unknown}/test"
      }
}"#
        .as_bytes(),
    );

    environment.clone().run_in_runtime(async move {
      let result = get_result("https://dprint.dev/test.json", &environment).await.err().unwrap();
      assert_eq!(
        result.to_string(),
        concat!(
          "Unknown template literal ${unknown}. Only ${configDir} and ${originConfigDir} are supported. If you meant to pass this to a plugin, escape the dollar sign with two back slashes.\n",
          "    at https://dprint.dev/test.json"
        ),
      );
    });
  }
}
