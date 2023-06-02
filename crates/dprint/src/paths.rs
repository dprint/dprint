use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::Split;
use thiserror::Error;

use crate::arg_parser::FilePatternArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_all_file_patterns;
use crate::plugins::Plugin;
use crate::plugins::PluginNameResolutionMaps;
use crate::utils::glob;

/// Struct that allows using plugin names as a key
/// in a hash map.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PluginNames(String);

impl PluginNames {
  const SEPARATOR: &'static str = "~~";

  pub fn from_plugin_names(names: &[String]) -> Self {
    Self(names.join(PluginNames::SEPARATOR))
  }

  pub fn names(&self) -> Split<'_, &str> {
    self.0.split(PluginNames::SEPARATOR)
  }
}

#[derive(Debug, Error)]
#[error("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.")]
pub struct NoFilesFoundError;

pub fn get_file_paths_by_plugins_and_err_if_empty(
  plugins: &[Box<dyn Plugin>],
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
  let result = get_file_paths_by_plugins(plugins, file_paths, config_base_path)?;
  if result.is_empty() {
    Err(NoFilesFoundError.into())
  } else {
    Ok(result)
  }
}

pub fn get_file_paths_by_plugins(
  plugins: &[Box<dyn Plugin>],
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
  let plugin_name_maps = PluginNameResolutionMaps::from_plugins(plugins, config_base_path)?;

  let mut file_paths_by_plugin: HashMap<PluginNames, Vec<PathBuf>> = HashMap::new();

  for file_path in file_paths.into_iter() {
    let plugin_names = plugin_name_maps.get_plugin_names_from_file_path(&file_path);

    if !plugin_names.is_empty() {
      let plugin_names_key = PluginNames::from_plugin_names(&plugin_names);
      let file_paths = file_paths_by_plugin.entry(plugin_names_key).or_insert_with(Vec::new);
      file_paths.push(file_path);
    }
  }

  Ok(file_paths_by_plugin)
}

pub async fn get_and_resolve_file_paths(config: &ResolvedConfig, args: &FilePatternArgs, environment: &impl Environment) -> Result<Vec<PathBuf>> {
  let cwd = environment.cwd();
  let file_patterns = get_all_file_patterns(config, args, &cwd);
  let is_in_sub_dir = cwd != config.base_path && cwd.starts_with(&config.base_path);
  let base_dir = if is_in_sub_dir { cwd } else { config.base_path.clone() };
  let environment = environment.clone();

  // This is intensive so do it in a blocking task
  // Eventually this could should maybe be changed to use tokio tasks
  tokio::task::spawn_blocking(move || glob(&environment, &base_dir, file_patterns)).await.unwrap()
}
