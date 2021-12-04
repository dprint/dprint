use std::collections::HashMap;
use std::path::PathBuf;
use std::str::Split;

use dprint_cli_core::types::ErrBox;

use crate::arg_parser::CliArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_all_file_patterns;
use crate::patterns::get_plugin_association_glob_matcher;
use crate::plugins::Plugin;
use crate::utils::glob;
use crate::utils::GlobPattern;
use crate::utils::GlobPatterns;

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
  plugins: &Vec<Box<dyn Plugin>>,
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>, ErrBox> {
  let result = get_file_paths_by_plugins(plugins, file_paths, config_base_path)?;
  if result.is_empty() {
    return err!("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.");
  }
  Ok(result)
}

pub fn get_file_paths_by_plugins(
  plugins: &Vec<Box<dyn Plugin>>,
  file_paths: Vec<PathBuf>,
  config_base_path: &CanonicalizedPathBuf,
) -> Result<HashMap<PluginNames, Vec<PathBuf>>, ErrBox> {
  let mut plugin_by_file_extension = HashMap::new();
  let mut plugin_by_file_name = HashMap::new();
  let mut plugin_associations = Vec::new();

  for plugin in plugins.iter() {
    for file_extension in plugin.file_extensions() {
      plugin_by_file_extension.entry(file_extension.to_lowercase()).or_insert(plugin.name());
    }
    for file_name in plugin.file_names() {
      plugin_by_file_name.entry(file_name.to_lowercase()).or_insert(plugin.name());
    }
    if let Some(matcher) = get_plugin_association_glob_matcher(plugin, config_base_path)? {
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
      plugin_names_key = if let Some(plugin) = crate::utils::get_lowercase_file_name(&file_path).and_then(|k| plugin_by_file_name.get(k.as_str())) {
        Some(PluginNames(plugin.to_string()))
      } else if let Some(plugin) = crate::utils::get_lowercase_file_extension(&file_path).and_then(|k| plugin_by_file_extension.get(k.as_str())) {
        Some(PluginNames(plugin.to_string()))
      } else {
        None
      };
    }

    if let Some(plugin_names_key) = plugin_names_key {
      let file_paths = file_paths_by_plugin.entry(plugin_names_key).or_insert(vec![]);
      file_paths.push(file_path);
    }
  }

  Ok(file_paths_by_plugin)
}

pub fn get_and_resolve_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
  let (file_patterns, absolute_paths) = get_config_file_paths(config, args, environment)?;
  return resolve_file_paths(file_patterns, &absolute_paths, args, config, environment);
}

fn get_config_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<(GlobPatterns, Vec<PathBuf>), ErrBox> {
  let cwd = environment.cwd();
  let mut file_patterns = get_all_file_patterns(config, args, &cwd);
  let absolute_paths = take_absolute_paths(&mut file_patterns.includes, environment);
  let absolute_paths = if args.file_patterns.is_empty() {
    // Filter out any config absolute file paths that don't exist.
    // This is to support sparse checkouts.
    absolute_paths.into_iter().filter(|file_path| environment.path_exists(file_path)).collect()
  } else {
    absolute_paths
  };

  return Ok((file_patterns, absolute_paths));
}

fn resolve_file_paths(
  file_patterns: GlobPatterns,
  absolute_paths: &Vec<PathBuf>,
  args: &CliArgs,
  config: &ResolvedConfig,
  environment: &impl Environment,
) -> Result<Vec<PathBuf>, ErrBox> {
  let cwd = environment.cwd();
  let is_in_sub_dir = cwd != config.base_path && cwd.starts_with(&config.base_path);
  if is_in_sub_dir {
    let mut file_paths = glob(environment, &cwd, file_patterns)?;
    if args.file_patterns.is_empty() {
      // filter file paths by cwd if no CLI paths are specified
      file_paths.extend(absolute_paths.iter().filter(|path| path.starts_with(&cwd)).map(ToOwned::to_owned));
    } else {
      file_paths.extend(absolute_paths.iter().map(ToOwned::to_owned));
    }
    Ok(file_paths)
  } else {
    let mut file_paths = glob(environment, &config.base_path, file_patterns)?;
    file_paths.extend(absolute_paths.clone());
    Ok(file_paths)
  }
}

pub fn take_absolute_paths(file_patterns: &mut Vec<GlobPattern>, environment: &impl Environment) -> Vec<PathBuf> {
  let len = file_patterns.len();
  let mut file_paths = Vec::new();
  for i in (0..len).rev() {
    let absolute_pattern = file_patterns[i].absolute_pattern();
    if is_absolute_path(&absolute_pattern, environment) {
      // can't use swap_remove because order is important
      file_patterns.remove(i);
      file_paths.push(PathBuf::from(absolute_pattern));
    }
  }
  file_paths
}

fn is_absolute_path(file_pattern: &str, environment: &impl Environment) -> bool {
  return !has_glob_chars(file_pattern) && environment.is_absolute_path(file_pattern);

  fn has_glob_chars(text: &str) -> bool {
    for c in text.chars() {
      match c {
        '*' | '{' | '}' | '[' | ']' | '!' => return true,
        _ => {}
      }
    }

    false
  }
}
