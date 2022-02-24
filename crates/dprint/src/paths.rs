use anyhow::bail;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::Split;

use crate::arg_parser::CliArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_all_file_patterns;
use crate::patterns::get_plugin_association_glob_matcher;
use crate::plugins::Plugin;
use crate::utils::glob;

/// Struct that allows using plugin names as a key
/// in a hash map.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PluginNames(String);

impl PluginNames {
  pub fn names(&self) -> Split<'_, &str> {
    self.0.split("~~")
  }

  fn add_plugin(&mut self, plugin_name: &str) {
    if !self.0.is_empty() {
      self.0.push_str("~~");
    }
    self.0.push_str(plugin_name);
  }
}

pub fn get_file_paths_by_plugins_and_err_if_empty(
  plugins: &[Box<dyn Plugin>],
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
  let result = get_file_paths_by_plugins(plugins, file_paths, config_base_path)?;
  if result.is_empty() {
    bail!("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.");
  }
  Ok(result)
}

pub fn get_file_paths_by_plugins(
  plugins: &[Box<dyn Plugin>],
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>> {
  let mut plugin_by_file_extension = HashMap::new();
  let mut plugin_by_file_name = HashMap::new();
  let mut plugin_associations = Vec::new();

  for plugin in plugins.iter() {
    for file_extension in plugin.file_extensions() {
      plugin_by_file_extension.entry(file_extension.to_lowercase()).or_insert_with(|| plugin.name());
    }
    for file_name in plugin.file_names() {
      plugin_by_file_name.entry(file_name.to_lowercase()).or_insert_with(|| plugin.name());
    }
    if let Some(matcher) = get_plugin_association_glob_matcher(&**plugin, config_base_path)? {
      plugin_associations.push((plugin.name(), matcher));
    }
  }

  let mut file_paths_by_plugin: HashMap<PluginNames, Vec<PathBuf>> = HashMap::new();

  for file_path in file_paths.into_iter() {
    let mut plugin_names_key: Option<PluginNames> = None;
    for (plugin_name, matcher) in plugin_associations.iter() {
      if matcher.is_match(&file_path) {
        if let Some(plugin_names_key) = plugin_names_key.as_mut() {
          plugin_names_key.add_plugin(plugin_name);
        } else {
          plugin_names_key = Some(PluginNames(plugin_name.to_string()));
        }
      }
    }
    if plugin_names_key.is_none() {
      plugin_names_key = {
        crate::utils::get_lowercase_file_name(&file_path)
          .and_then(|k| plugin_by_file_name.get(k.as_str()))
          .or_else(|| crate::utils::get_lowercase_file_extension(&file_path).and_then(|k| plugin_by_file_extension.get(k.as_str())))
          .map(|plugin| PluginNames(plugin.to_string()))
      };
    }

    if let Some(plugin_names_key) = plugin_names_key {
      let file_paths = file_paths_by_plugin.entry(plugin_names_key).or_insert_with(Vec::new);
      file_paths.push(file_path);
    }
  }

  Ok(file_paths_by_plugin)
}

pub fn get_and_resolve_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>> {
  let cwd = environment.cwd();
  let file_patterns = get_all_file_patterns(config, args, &cwd);
  let is_in_sub_dir = cwd != config.base_path && cwd.starts_with(&config.base_path);
  let base_dir = if is_in_sub_dir { &cwd } else { &config.base_path };
  glob(environment, &base_dir, file_patterns)
}
