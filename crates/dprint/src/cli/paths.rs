use std::collections::HashMap;
use std::path::PathBuf;

use dprint_cli_core::types::ErrBox;

use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::utils::glob;

use super::configuration::ResolvedConfig;
use super::patterns::get_all_file_patterns;
use super::CliArgs;

pub fn get_file_paths_by_plugin_and_err_if_empty(plugins: &Vec<Box<dyn Plugin>>, file_paths: Vec<PathBuf>) -> Result<HashMap<String, Vec<PathBuf>>, ErrBox> {
  let file_paths_by_plugin = get_file_paths_by_plugin(plugins, file_paths);
  if file_paths_by_plugin.is_empty() {
    return err!("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.");
  }
  Ok(file_paths_by_plugin)
}

pub fn get_file_paths_by_plugin(plugins: &Vec<Box<dyn Plugin>>, file_paths: Vec<PathBuf>) -> HashMap<String, Vec<PathBuf>> {
  let mut plugin_by_file_extension: HashMap<&str, &str> = HashMap::new();
  let mut plugin_by_file_name: HashMap<&str, &str> = HashMap::new();

  for plugin in plugins.iter() {
    for file_extension in plugin.file_extensions() {
      plugin_by_file_extension.entry(file_extension).or_insert(plugin.name());
    }
    for file_name in plugin.file_names() {
      plugin_by_file_name.entry(file_name).or_insert(plugin.name());
    }
  }

  let mut file_paths_by_plugin: HashMap<String, Vec<PathBuf>> = HashMap::new();

  for file_path in file_paths.into_iter() {
    let plugin = if let Some(plugin) = crate::utils::get_lowercase_file_name(&file_path).and_then(|k| plugin_by_file_name.get(k.as_str())) {
      plugin
    } else if let Some(plugin) = crate::utils::get_lowercase_file_extension(&file_path).and_then(|k| plugin_by_file_extension.get(k.as_str())) {
      plugin
    } else {
      continue;
    };
    let file_paths = file_paths_by_plugin.entry(plugin.to_string()).or_insert(vec![]);
    file_paths.push(file_path);
  }

  file_paths_by_plugin
}

pub fn get_and_resolve_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
  let (file_patterns, absolute_paths) = get_config_file_paths(config, args, environment)?;
  return resolve_file_paths(&file_patterns, &absolute_paths, args, config, environment);
}

fn get_config_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<(Vec<String>, Vec<PathBuf>), ErrBox> {
  let cwd = environment.cwd();
  let mut file_patterns = get_all_file_patterns(config, args, &cwd.to_string_lossy());
  let absolute_paths = take_absolute_paths(&mut file_patterns, environment);
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
  file_patterns: &Vec<String>,
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
    return Ok(file_paths);
  } else {
    let mut file_paths = glob(environment, &config.base_path, file_patterns)?;
    file_paths.extend(absolute_paths.clone());
    return Ok(file_paths);
  }
}

pub fn take_absolute_paths(file_patterns: &mut Vec<String>, environment: &impl Environment) -> Vec<PathBuf> {
  let len = file_patterns.len();
  let mut file_paths = Vec::new();
  for i in (0..len).rev() {
    if is_absolute_path(&file_patterns[i], environment) {
      file_paths.push(PathBuf::from(file_patterns.swap_remove(i))); // faster
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
