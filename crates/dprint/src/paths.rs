use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::Split;
use thiserror::Error;

use crate::arg_parser::FilePatternArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_all_file_patterns;
use crate::patterns::process_config_patterns;
use crate::plugins::PluginNameResolutionMaps;
use crate::resolution::PluginWithConfig;
use crate::utils::glob;
use crate::utils::is_negated_glob;
use crate::utils::GlobOutput;
use crate::utils::GlobPattern;

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
#[error("No files found to format with the specified plugins at {}. You may want to try using `dprint output-file-paths` to see which files it's finding.", .base_path.display())]
pub struct NoFilesFoundError {
  pub base_path: CanonicalizedPathBuf,
}

pub fn get_file_paths_by_plugins_and_err_if_empty(
  base_path: &CanonicalizedPathBuf,
  plugin_name_maps: &PluginNameResolutionMaps,
  file_paths: Vec<PathBuf>,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
  let result = get_file_paths_by_plugins(plugin_name_maps, file_paths)?;
  if result.is_empty() {
    Err(NoFilesFoundError {
      base_path: base_path.clone(),
    }.into())
  } else {
    Ok(result)
  }
}

pub fn get_file_paths_by_plugins(plugin_name_maps: &PluginNameResolutionMaps, file_paths: Vec<PathBuf>) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
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

pub async fn get_and_resolve_file_paths<'a>(
  config: &ResolvedConfig,
  args: &FilePatternArgs,
  plugins: impl Iterator<Item = &'a PluginWithConfig>,
  environment: &impl Environment,
) -> Result<GlobOutput> {
  let cwd = environment.cwd();
  let mut file_patterns = get_all_file_patterns(config, args, &cwd);
  if file_patterns.includes.is_none() {
    // If no includes patterns were specified, derive one from the list of plugins
    // as this is a massive performance improvement, because it collects less file
    // paths to examine and match to plugins later.
    file_patterns.includes = Some(GlobPattern::new_vec(get_plugin_patterns(plugins), cwd.clone()));
  }
  let is_in_sub_dir = cwd != config.base_path && cwd.starts_with(&config.base_path);
  let base_dir = if is_in_sub_dir { cwd } else { config.base_path.clone() };
  let environment = environment.clone();

  // This is intensive so do it in a blocking task
  dprint_core::async_runtime::spawn_blocking(move || glob(&environment, &base_dir, file_patterns))
    .await
    .unwrap()
}

fn get_plugin_patterns<'a>(plugins: impl Iterator<Item = &'a PluginWithConfig>) -> Vec<String> {
  let mut file_names = HashSet::new();
  let mut file_exts = HashSet::new();
  let mut association_globs = Vec::new();
  for plugin in plugins {
    let mut had_positive_association = false;
    if let Some(associations) = plugin.associations.as_ref() {
      for pattern in process_config_patterns(associations) {
        if !is_negated_glob(&pattern) {
          had_positive_association = true;
          association_globs.push(pattern);
        }
      }
    }
    if !had_positive_association {
      file_names.extend(&plugin.info().file_names);
      file_exts.extend(&plugin.info().file_extensions);
    }
  }
  let mut result = Vec::new();
  if !file_exts.is_empty() {
    result.push(format!("**/*.{{{}}}", file_exts.into_iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
  }
  if !file_names.is_empty() {
    result.push(format!("**/{{{}}}", file_names.into_iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
  }
  // add the association globs last as they're least likely to be matched
  result.extend(association_globs);

  result
}
