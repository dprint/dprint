use dprint_cli_core::types::ErrBox;

use crate::cache::Cache;
use crate::configuration::{get_global_config, get_plugin_config_map};
use crate::environment::Environment;
use crate::plugins::{Plugin, PluginResolver};

use super::CliArgs;
use super::configuration::{ResolvedConfig, resolve_config_from_args};

pub fn get_plugins_from_args<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
  match resolve_config_from_args(args, cache, environment) {
      Ok(config) => resolve_plugins(&config, environment, plugin_resolver),
      Err(_) => Ok(Vec::new()), // ignore
  }
}

pub fn resolve_plugins_and_err_if_empty<TEnvironment: Environment>(
  config: &ResolvedConfig,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
  let plugins = resolve_plugins(config, environment, plugin_resolver)?;
  if plugins.is_empty() {
      return err!("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
  }
  Ok(plugins)
}

pub fn resolve_plugins<TEnvironment: Environment>(
  config: &ResolvedConfig,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
  // resolve the plugins
  let plugins = plugin_resolver.resolve_plugins(config.plugins.clone())?;
  let mut config_map = config.config_map.clone();

  // resolve each plugin's configuration
  let mut plugins_with_config = Vec::new();
  for plugin in plugins.into_iter() {
      plugins_with_config.push((
          get_plugin_config_map(&plugin, &mut config_map)?,
          plugin
      ));
  }

  // now get global config
  let global_config = get_global_config(config_map, environment)?;

  // now set each plugin's config
  let mut plugins = Vec::new();
  for (plugin_config, plugin) in plugins_with_config {
      let mut plugin = plugin;
      plugin.set_config(plugin_config, global_config.clone());
      plugins.push(plugin);
  }

  return Ok(plugins);
}
