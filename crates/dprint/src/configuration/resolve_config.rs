use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
use anyhow::bail;
use deno_terminal::colors;
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
use crate::utils::GlobPattern;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::ResolvedFilePathWithText;
use crate::utils::ResolvedFilePathWithTextRef;
use crate::utils::ShowConfirmStrategy;
use crate::utils::resolve_url_or_file_path_to_file_with_cache;

use super::resolve_main_config_path::ResolvedConfigPathWithText;
use super::resolve_main_config_path::resolve_main_config_path_and_bytes;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedConfig {
  pub source: PathSource,
  /// The folder that should be considered the "root".
  pub base_path: CanonicalizedPathBuf,
  /// Whether this is the user's global configuration file.
  pub is_global: bool,
  pub includes: Option<Vec<String>>,
  pub excludes: Option<Vec<String>>,
  pub plugins: Vec<PluginSourceReference>,
  pub incremental: Option<bool>,
  /// Whether a nested (directory specific) configuration file should inherit
  /// the plugins and configuration of its ancestor configuration file.
  pub inherit: Option<bool>,
  pub config_map: ConfigMap,
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum ResolveConfigError {
  #[error(
    "No config file found at {}. Did you mean to create (dprint init) or specify one (--config <path>)?\n\n{}",
    .config_path.display(),
    colors::gray("Note: dprint now supports global configuration. Set it up with `dprint init --global` then edit with `dprint config edit --global`")
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
        colors::yellow("Warning"),
        self.directory.display(),
        match selected {
          Some(true) => "Y",
          Some(false) => "N",
          None => "(Y/n) \u{2588}",
        },
        match selected {
          Some(_) => colors::gray(""),
          None => colors::gray("\n\nHint: Specify the directory to bypass this prompt in the future (ex. `dprint fmt .`)"),
        },
      )
    }

    fn default_value(&self) -> bool {
      true
    }
  }

  let config_path_and_bytes = resolve_main_config_path_and_bytes(args, environment).await?;
  let mut resolved_config = match config_path_and_bytes {
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
      resolve_config_from_path_with_bytes(&resolved_config_path, environment).await?
    }
    None => {
      if !args.plugins.is_empty() {
        // allow no config file when plugins are specified
        ResolvedConfig {
          config_map: ConfigMap::new(),
          base_path: environment.cwd().clone(),
          source: PathSource::new_local(environment.cwd().join_panic_relative("dprint.json")),
          is_global: false,
          excludes: None,
          includes: None,
          incremental: None,
          inherit: None,
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

pub async fn resolve_config_from_path_with_bytes<TEnvironment: Environment>(
  config_path_and_text: &ResolvedConfigPathWithText,
  environment: &TEnvironment,
) -> Result<ResolvedConfig, ResolveConfigError> {
  let base_source = config_path_and_text.source.parent();
  let mut config_map = get_config_map_from_path(ConfigPathContext {
    current: config_path_and_text.as_file_path_with_text_ref(),
    origin: &config_path_and_text.source,
  })
  .map_err(|err| anyhow::anyhow!("{:#}\n    at {}", err, config_path_and_text.source.display()))?;

  let plugins_vec = take_plugins_array_from_config_map(&mut config_map, &base_source, environment)?; // always take this out of the config map
  let plugins = filter_duplicate_plugin_sources({
    // filter out any non-wasm plugins from remote config
    if !config_path_and_text.source.is_local() {
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
  if !config_path_and_text.source.is_local() {
    // Careful! Don't be fancy and ensure this is removed.
    let removed_includes = config_map.shift_remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && config_path_and_text.is_first_download {
      log_warn!(environment, &get_warn_includes_message());
    }
  }
  // =========

  let includes = take_array_from_config_map(&mut config_map, "includes")?;
  let excludes = take_array_from_config_map(&mut config_map, "excludes")?;

  let incremental = take_bool_from_config_map(&mut config_map, "incremental")?;
  let inherit = take_bool_from_config_map(&mut config_map, "inherit")?;
  config_map.shift_remove("projectType"); // this was an old config property that's no longer used
  let extends = take_extends(&mut config_map)?;
  let resolved_config = ResolvedConfig {
    source: config_path_and_text.source.clone(),
    base_path: config_path_and_text.base_path.clone(),
    is_global: config_path_and_text.is_global_config,
    config_map,
    includes,
    excludes,
    plugins,
    incremental,
    inherit,
  };

  // resolve extends
  Ok(resolve_extends(resolved_config, extends, base_source, environment.clone()).await?)
}

/// Merges the ancestor (`parent`) configuration into a nested configuration
/// file that has specified `"inherit": true`.
///
/// The nested config's values take precedence. Plugins specified in the nested
/// config have precedence over the ancestor's plugins, the ancestor's excludes
/// are combined with the nested config's excludes, and plugin configurations are
/// merged with the nested config winning on conflicts.
///
/// Note: `includes` are not inherited.
pub fn inherit_config(mut config: ResolvedConfig, parent: &ResolvedConfig) -> Result<ResolvedConfig> {
  // plugins specified in the nested config have precedence over the ancestor's
  config.plugins.extend(parent.plugins.iter().cloned());
  config.plugins = filter_duplicate_plugin_sources(std::mem::take(&mut config.plugins));

  // combine excludes, rebasing the ancestor's patterns onto this config's directory
  config.excludes = inherit_excludes(config.excludes, parent.excludes.as_deref(), &parent.base_path, &config.base_path);

  // inherit the incremental flag when not specified in the nested config
  if config.incremental.is_none() {
    config.incremental = parent.incremental;
  }

  merge_config_map_into(&mut config.config_map, parent.config_map.clone())?;

  Ok(config)
}

/// Combines a nested config's own excludes with its ancestor's, rebasing each
/// ancestor pattern from the ancestor's directory onto the nested config's
/// directory. Ancestor patterns that don't reach into the nested directory are
/// dropped.
///
/// The ancestor's patterns are ordered first so the nested config's own excludes
/// can opt back out of them (ex. with a `!` pattern).
fn inherit_excludes(
  own: Option<Vec<String>>,
  ancestor: Option<&[String]>,
  ancestor_base: &CanonicalizedPathBuf,
  new_base: &CanonicalizedPathBuf,
) -> Option<Vec<String>> {
  let Some(ancestor) = ancestor else {
    return own;
  };
  let mut result = ancestor
    .iter()
    .filter_map(|pattern| {
      GlobPattern::new(pattern.clone(), ancestor_base.clone())
        .into_new_base(new_base.clone())
        .map(|p| p.relative_pattern)
    })
    .collect::<Vec<_>>();
  if let Some(own) = own {
    result.extend(own);
  }
  if result.is_empty() { None } else { Some(result) }
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
      let resolved_file = resolve_url_or_file_path_to_file_with_cache(&url_or_file_path, &base_path, &environment)
        .await?
        .into_text()?;
      resolved_config = match handle_config_file(&resolved_file, resolved_config, &environment).await {
        Ok(resolved_config) => resolved_config,
        Err(err) => bail!("{:#}\n    at {}", err, resolved_file.source.display()),
      }
    }
    Ok(resolved_config)
  }
  .boxed_local()
}

async fn handle_config_file<TEnvironment: Environment>(
  config_path_and_text: &ResolvedFilePathWithText,
  mut resolved_config: ResolvedConfig,
  environment: &TEnvironment,
) -> Result<ResolvedConfig> {
  let mut new_config_map = get_config_map_from_path(ConfigPathContext {
    current: config_path_and_text.as_ref(),
    origin: &resolved_config.source,
  })?;
  let extends = take_extends(&mut new_config_map)?;

  // Discard any properties that shouldn't be inherited
  if !config_path_and_text.source.is_local() {
    // IMPORTANT
    // =========
    // Remove the includes from all referenced remote configuration since
    // we don't want it specifying something like system or some configuration
    // files that it could change. Basically, the end user should have 100%
    // control over what files get formatted.
    let removed_includes = new_config_map.shift_remove("includes"); // NEVER REMOVE THIS STATEMENT
    if removed_includes.is_some() && config_path_and_text.is_first_download {
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
  let plugins = take_plugins_array_from_config_map(&mut new_config_map, &config_path_and_text.source.parent(), environment)?;
  let plugins = if !config_path_and_text.source.is_local() {
    filter_non_wasm_plugins(plugins, environment)
  } else {
    plugins
  };
  // =========

  // combine plugins
  resolved_config.plugins.extend(plugins);

  merge_config_map_into(&mut resolved_config.config_map, new_config_map)?;

  resolve_extends(resolved_config, extends, config_path_and_text.source.parent(), environment.clone()).await
}

/// Merges the lower precedence `source` config map into the higher precedence
/// `target` config map. Values already present in `target` win, while plugin
/// configurations have their properties and overrides combined.
///
/// This is used both when resolving `extends` (the extended config is the lower
/// precedence `source`) and when a nested config `inherit`s its ancestor (the
/// ancestor config is the lower precedence `source`).
fn merge_config_map_into(target: &mut ConfigMap, source: ConfigMap) -> Result<()> {
  for (key, value) in source {
    match value {
      ConfigMapValue::KeyValue(key_value) => {
        target.entry(key).or_insert(ConfigMapValue::KeyValue(key_value));
      }
      ConfigMapValue::Vec(items) => {
        target.entry(key).or_insert(ConfigMapValue::Vec(items));
      }
      ConfigMapValue::PluginConfig(obj) => {
        if let Some(target_obj) = target.get_mut(&key) {
          if let ConfigMapValue::PluginConfig(target_obj) = target_obj {
            // check for locked configuration
            if obj.locked && (!target_obj.properties.is_empty() || !target_obj.overrides.is_empty()) {
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
              target_obj.properties.entry(key).or_insert(value);
            }

            if !obj.overrides.is_empty() {
              let mut overrides = obj.overrides;
              overrides.append(&mut target_obj.overrides);
              target_obj.overrides = overrides;
            }

            // Set the associations if they aren't overwritten in the higher
            // precedence config. This is ok to do because process plugins and
            // includes/excludes aren't inherited from other config.
            if target_obj.associations.is_none() {
              target_obj.associations = obj.associations;
            }
          }
        } else {
          target.insert(key, ConfigMapValue::PluginConfig(obj));
        }
      }
    }
  }
  Ok(())
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
  current: ResolvedFilePathWithTextRef<'a>,
  /// The original configuration file that may have extended
  /// the current configuration file.
  origin: &'a PathSource,
}

fn get_config_map_from_path(path: ConfigPathContext) -> Result<ConfigMap> {
  let mut result = match deserialize_config(path.current.content) {
    Ok(map) => map,
    Err(e) => bail!("Error deserializing. {}", e),
  };
  template_expand(path, &mut result)?;

  Ok(result)
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
          "configDir" => match &path.current.source {
            PathSource::Local(source) => {
              parts.push(Cow::Owned(source.path.parent().unwrap().to_string_lossy().to_string()));
            }
            PathSource::Remote(_) | PathSource::Npm(_) => {
              bail!("Cannot use ${{configDir}} template in remote configuration files. Maybe use ${{originConfigDir}} instead?");
            }
          },
          "originConfigDir" => match &path.origin {
            PathSource::Local(origin) => {
              parts.push(Cow::Owned(origin.path.parent().unwrap().to_string_lossy().to_string()));
            }
            PathSource::Remote(origin) => {
              bail!(
                "Cannot use ${{originConfigDir}} template when the origin configuration file ({}) is remote.",
                origin.url,
              );
            }
            PathSource::Npm(npm) => {
              bail!(
                "Cannot use ${{originConfigDir}} template when the origin configuration file ({}) is an npm package.",
                npm.specifier.display(),
              );
            }
          },
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
    colors::bold("Note: "),
  )
}

fn get_warn_non_wasm_plugins_message() -> String {
  format!(
    "{} Non-wasm plugins are ignored for security reasons on remote configuration.",
    colors::bold("Note: "),
  )
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
  use crate::configuration::RawPluginConfigOverride;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
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

  async fn resolve_local_config(path: &str, environment: &TestEnvironment) -> ResolvedConfig {
    let canonical = environment.canonicalize(path).unwrap();
    let config_path = ResolvedConfigPathWithText {
      content: environment.read_file(&canonical).unwrap(),
      base_path: canonical.parent().unwrap(),
      source: PathSource::new_local(canonical),
      is_global_config: false,
      is_first_download: false,
    };
    resolve_config_from_path_with_bytes(&config_path, environment).await.unwrap()
  }

  #[test]
  fn inherit_config_should_keep_config_dir_relative_to_each_config_file() {
    // ${configDir} is expanded when each config file is parsed (before the inherit
    // merge), so an inherited value keeps the ancestor's directory rather than being
    // re-based to the nested config file's directory.
    let environment = TestEnvironmentBuilder::new()
      .write_file(
        "/a/dprint.json",
        r#"{
            "test": {
              "fromAncestor": "${configDir}/value"
            }
        }"#,
      )
      .write_file(
        "/a/b/dprint.json",
        r#"{
            "inherit": true,
            "test": {
              "fromNested": "${configDir}/value"
            }
        }"#,
      )
      .build();

    environment.clone().run_in_runtime(async move {
      let parent = resolve_local_config("/a/dprint.json", &environment).await;
      let child = resolve_local_config("/a/b/dprint.json", &environment).await;
      let result = inherit_config(child, &parent).unwrap();
      assert_eq!(
        result.config_map,
        ConfigMap::from([(
          "test".to_string(),
          ConfigMapValue::PluginConfig(RawPluginConfig {
            locked: false,
            associations: None,
            overrides: Vec::new(),
            properties: ConfigKeyMap::from([
              // the nested config file's ${configDir} points at its own directory...
              ("fromNested".to_string(), ConfigKeyValue::from_str("/a/b/value")),
              // ...while the inherited value still points at the ancestor's directory
              ("fromAncestor".to_string(), ConfigKeyValue::from_str("/a/value")),
            ]),
          }),
        )])
      );
    });
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
      assert_eq!(result.source.is_local(), true);
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
      assert_eq!(result.source.is_remote(), true);
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
      assert_eq!(result.source.is_local(), true);
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
            overrides: Vec::new(),
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
            overrides: Vec::new(),
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
            overrides: Vec::new(),
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
            overrides: Vec::new(),
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
            overrides: Vec::new(),
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
            overrides: Vec::new(),
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
    let environment = TestEnvironmentBuilder::new()
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#,
      )
      .write_file(
        &PathBuf::from("/dir/test.json"),
        r#"{
            "extends": "../otherDir/test.json",
            "prop2": 2
        }"#,
      )
      .write_file(
        &PathBuf::from("/otherDir/test.json"),
        r#"{
            "prop3": 3
        }"#,
      )
      .build();

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
          overrides: Vec::new(),
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
          overrides: Vec::new(),
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
          overrides: Vec::new(),
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
  fn should_use_overrides_on_extended_config() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
          "test": {
            "overrides": {
              "files": "**/package.json",
              "lineWidth": 80
            }
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
          overrides: vec![RawPluginConfigOverride {
            files: vec!["**/package.json".to_string()],
            properties: ConfigKeyMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(80))]),
          }],
          properties: ConfigKeyMap::new(),
        }),
      )]);

      assert_eq!(result.config_map, expected_config_map);
    });
  }

  #[test]
  fn should_order_extended_overrides_before_local_overrides() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
          "test": {
            "overrides": {
              "files": "**/*.json",
              "lineWidth": 100
            }
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
              "overrides": {
                "files": "**/package.json",
                "lineWidth": 80
              }
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
          associations: None,
          overrides: vec![
            RawPluginConfigOverride {
              files: vec!["**/*.json".to_string()],
              properties: ConfigKeyMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(100))]),
            },
            RawPluginConfigOverride {
              files: vec!["**/package.json".to_string()],
              properties: ConfigKeyMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(80))]),
            },
          ],
          properties: ConfigKeyMap::new(),
        }),
      )]);

      assert_eq!(result.config_map, expected_config_map);
    });
  }

  #[test]
  fn should_error_extending_locked_config_with_overrides() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      "https://dprint.dev/test.json",
      r#"{
          "test": {
            "locked": true,
            "lineWidth": 80
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
              "overrides": {
                "files": "**/package.json",
                "lineWidth": 100
              }
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
          overrides: Vec::new(),
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
          overrides: Vec::new(),
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
    let environment = TestEnvironmentBuilder::new()
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "./other/test.json",
        }"#,
      )
      .write_file(
        &PathBuf::from("/other/test.json"),
        r#"{
            "projectType": "openSource", // test having this in base config
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .build();

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
  fn should_parse_inherit_property() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "inherit": true,
            "plugins": ["./testing/asdf.wasm"],
        }"#,
      )
      .unwrap();

    environment.clone().run_in_runtime(async move {
      let result = get_result("/test.json", &environment).await.unwrap();
      assert_eq!(environment.take_stdout_messages().len(), 0);
      assert_eq!(result.inherit, Some(true));
      // should not leak into the config map (which would cause an unknown property diagnostic)
      assert_eq!(result.config_map.contains_key("inherit"), false);
    });
  }

  #[test]
  fn inherit_config_should_merge_ancestor_config() {
    let parent = ResolvedConfig {
      source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/dprint.json")),
      base_path: CanonicalizedPathBuf::new_for_testing("/"),
      is_global: false,
      includes: Some(vec!["**/*.txt".to_string()]),
      // "**/node_modules" rebases into the nested directory, but the anchored
      // "dist" points outside it and is dropped
      excludes: Some(vec!["**/node_modules".to_string(), "dist".to_string()]),
      plugins: vec![
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/json.wasm"),
      ],
      incremental: Some(true),
      inherit: None,
      config_map: ConfigMap::from([
        ("lineWidth".to_string(), ConfigMapValue::from_i32(80)),
        (
          "test".to_string(),
          ConfigMapValue::PluginConfig(RawPluginConfig {
            locked: false,
            associations: None,
            overrides: Vec::new(),
            properties: ConfigKeyMap::from([
              ("indentWidth".to_string(), ConfigKeyValue::from_i32(4)),
              ("newLineKind".to_string(), ConfigKeyValue::from_str("crlf")),
            ]),
          }),
        ),
      ]),
    };
    let child = ResolvedConfig {
      source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/sub/dprint.json")),
      base_path: CanonicalizedPathBuf::new_for_testing("/sub"),
      is_global: false,
      includes: None,
      excludes: Some(vec!["sub-excludes".to_string()]),
      // a plugin specified in the child has precedence over the ancestor's
      plugins: vec![PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm")],
      incremental: None,
      inherit: Some(true),
      config_map: ConfigMap::from([(
        "test".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          overrides: Vec::new(),
          properties: ConfigKeyMap::from([("indentWidth".to_string(), ConfigKeyValue::from_i32(2))]),
        }),
      )]),
    };

    let result = inherit_config(child, &parent).unwrap();
    // child plugins first, then ancestor's, with duplicates removed
    assert_eq!(
      result.plugins,
      vec![
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm"),
        PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/json.wasm"),
      ]
    );
    // ancestor excludes (rebased, droppable) come first, then the nested config's own
    assert_eq!(result.excludes, Some(vec!["**/node_modules".to_string(), "sub-excludes".to_string()]));
    // includes are not inherited
    assert_eq!(result.includes, None);
    // incremental is inherited when not specified
    assert_eq!(result.incremental, Some(true));
    assert_eq!(
      result.config_map,
      ConfigMap::from([
        (
          "test".to_string(),
          ConfigMapValue::PluginConfig(RawPluginConfig {
            locked: false,
            associations: None,
            overrides: Vec::new(),
            properties: ConfigKeyMap::from([
              // child wins on conflicts...
              ("indentWidth".to_string(), ConfigKeyValue::from_i32(2)),
              // ...but inherits values it didn't override
              ("newLineKind".to_string(), ConfigKeyValue::from_str("crlf")),
            ]),
          }),
        ),
        ("lineWidth".to_string(), ConfigMapValue::from_i32(80)),
      ])
    );
  }

  #[test]
  fn inherit_config_should_rebase_ancestor_excludes_onto_nested_directory() {
    fn inherited_excludes(ancestor: &[&str], ancestor_base: &str, new_base: &str) -> Option<Vec<String>> {
      inherit_excludes(
        None,
        Some(&ancestor.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        &CanonicalizedPathBuf::new_for_testing(ancestor_base),
        &CanonicalizedPathBuf::new_for_testing(new_base),
      )
    }

    // depth-relative patterns keep matching within the nested directory
    assert_eq!(inherited_excludes(&["**/node_modules"], "/", "/sub"), Some(vec!["**/node_modules".to_string()]));
    // a pattern anchored into the nested directory is rebased to be relative to it
    assert_eq!(inherited_excludes(&["sub/dist"], "/", "/sub"), Some(vec!["dist".to_string()]));
    // an anchored pattern that points outside the nested directory is dropped
    assert_eq!(inherited_excludes(&["other/dist"], "/", "/sub"), None);
    // dropping leaves the nested config's own excludes intact
    assert_eq!(
      inherit_excludes(
        Some(vec!["own".to_string()]),
        Some(&["other/dist".to_string()]),
        &CanonicalizedPathBuf::new_for_testing("/"),
        &CanonicalizedPathBuf::new_for_testing("/sub"),
      ),
      Some(vec!["own".to_string()])
    );
  }

  #[test]
  fn inherit_config_should_error_overriding_locked_ancestor_config() {
    let parent = ResolvedConfig {
      source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/dprint.json")),
      base_path: CanonicalizedPathBuf::new_for_testing("/"),
      is_global: false,
      includes: None,
      excludes: None,
      plugins: Vec::new(),
      incremental: None,
      inherit: None,
      config_map: ConfigMap::from([(
        "test".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: true,
          associations: None,
          overrides: Vec::new(),
          properties: ConfigKeyMap::from([("indentWidth".to_string(), ConfigKeyValue::from_i32(4))]),
        }),
      )]),
    };
    let child = ResolvedConfig {
      source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/sub/dprint.json")),
      base_path: CanonicalizedPathBuf::new_for_testing("/sub"),
      is_global: false,
      includes: None,
      excludes: None,
      plugins: Vec::new(),
      incremental: None,
      inherit: Some(true),
      config_map: ConfigMap::from([(
        "test".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          locked: false,
          associations: None,
          overrides: Vec::new(),
          properties: ConfigKeyMap::from([("indentWidth".to_string(), ConfigKeyValue::from_i32(2))]),
        }),
      )]),
    };

    let err = inherit_config(child, &parent).err().unwrap();
    assert_eq!(
      err.to_string(),
      concat!(
        "The configuration for \"test\" was locked, but a parent configuration specified it. ",
        "Locked configurations cannot have their properties overridden."
      )
    );
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
    let environment = TestEnvironmentBuilder::new()
      .write_file(
        &PathBuf::from("/test.json"),
        r#"{
            "extends": "dir/test.json",
            "prop1": 1
        }"#,
      )
      .write_file(
        &PathBuf::from("/dir/test.json"),
        r#"{
            "plugins": ["./test-plugin.json@checksum"]
        }"#,
      )
      .build();

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
    let environment = TestEnvironmentBuilder::new()
      .add_remote_file(
        "https://dprint.dev/test.json",
        r#"{
      "extends": "./next.json",
      "otherPlugin": {
        "value": "${originConfigDir}/origin"
      }
}"#,
      )
      .add_remote_file(
        "https://dprint.dev/next.json",
        r#"{
      "final": {
        "value": "${originConfigDir}/final && \\${configDir}/escaped"
      }
}"#,
      )
      .write_file(
        "/dir/dprint.json",
        r#"{
      "extends": "https://dprint.dev/test.json",
      "plugin": {
        "value": "${configDir}/test && ${originConfigDir}/other"
      }
}"#,
      )
      .build();

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
              overrides: Vec::new(),
              properties: ConfigKeyMap::from([(String::from("value"), ConfigKeyValue::from_str("/dir/test && /dir/other"))]),
            }),
          ),
          (
            "otherPlugin".to_string(),
            ConfigMapValue::PluginConfig(RawPluginConfig {
              locked: false,
              associations: None,
              overrides: Vec::new(),
              properties: ConfigKeyMap::from([(String::from("value"), ConfigKeyValue::from_str("/dir/origin"))]),
            }),
          ),
          (
            "final".to_string(),
            ConfigMapValue::PluginConfig(RawPluginConfig {
              locked: false,
              associations: None,
              overrides: Vec::new(),
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
