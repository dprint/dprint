use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::arg_parser::CliArgs;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::resolve_config_from_args;
use crate::environment::Environment;
use crate::paths::get_and_resolve_file_paths;
use crate::paths::get_file_paths_by_plugins_and_err_if_empty;
use crate::paths::PluginNames;
use crate::plugins::resolve_plugins_and_err_if_empty;
use crate::plugins::Plugin;
use crate::plugins::PluginResolver;

pub struct PluginsAndPaths {
  pub plugins: Vec<Box<dyn Plugin>>,
  pub file_paths_by_plugins: HashMap<PluginNames, Vec<PathBuf>>,
}

pub async fn resolve_plugins_and_paths<TEnvironment: Environment>(
  args: &CliArgs,
  patterns: &FilePatternArgs,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<PluginsAndPaths> {
  let config = resolve_config_from_args(args, environment)?;
  let plugins = resolve_plugins_and_err_if_empty(args, &config, environment, plugin_resolver).await?;
  let glob_output = get_and_resolve_file_paths(&config, patterns, &plugins, environment).await?;
  let file_paths_by_plugins = get_file_paths_by_plugins_and_err_if_empty(&plugins, glob_output.file_paths, &config.base_path)?;
  Ok(PluginsAndPaths {
    plugins,
    file_paths_by_plugins,
  })
}
