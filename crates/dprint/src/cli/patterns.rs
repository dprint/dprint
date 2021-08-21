use std::path::Path;

use dprint_cli_core::types::ErrBox;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::environment::Environment;
use crate::utils::to_absolute_globs;

use super::configuration::ResolvedConfig;
use super::paths::take_absolute_paths;
use super::CliArgs;

pub struct FileMatcher {
  include_globset: GlobSet,
  exclude_globset: GlobSet,
}

impl FileMatcher {
  pub fn new(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Self, ErrBox> {
    let cwd = environment.cwd()?;
    let cwd_str = cwd.to_string_lossy();
    let mut include_file_patterns = get_include_file_patterns(&config, args, &cwd_str);
    let include_globset = build_glob_set(config, environment, &mut include_file_patterns)?;

    let mut exclude_file_patterns = get_exclude_file_patterns(&config, args, &cwd_str);
    let exclude_globset = build_glob_set(config, environment, &mut exclude_file_patterns)?;

    Ok(FileMatcher {
      include_globset,
      exclude_globset,
    })
  }

  pub fn matches(&self, file_path: &Path) -> bool {
    let matches_includes = self.include_globset.is_match(&file_path);
    let matches_excludes = self.exclude_globset.is_match(&file_path);
    matches_includes && !matches_excludes
  }
}

fn build_glob_set<TEnvironment: Environment>(config: &ResolvedConfig, environment: &TEnvironment, file_patterns: &mut Vec<String>) -> Result<GlobSet, ErrBox> {
  let mut builder = GlobSetBuilder::new();
  let cwd = environment.cwd()?;
  let config_base_path = config.base_path.to_string_lossy();
  let base_path = match config_base_path.as_ref() {
    "./" => cwd.to_string_lossy(),
    _ => config_base_path,
  };
  let absolute_paths = take_absolute_paths(file_patterns, environment);
  for pattern in to_absolute_globs(file_patterns, &base_path) {
    builder.add(Glob::new(&pattern)?);
  }
  for path in absolute_paths {
    let path_as_str = path.to_string_lossy();
    builder.add(Glob::new(&path_as_str)?);
  }
  return Ok(builder.build().unwrap());
}

pub fn get_all_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut include_file_patterns = get_include_file_patterns(config, args, cwd);
  let mut exclude_file_patterns = get_exclude_file_patterns(config, args, cwd);
  include_file_patterns.append(&mut exclude_file_patterns);
  return include_file_patterns;
}

fn get_include_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(if args.file_patterns.is_empty() {
    config.includes.clone()
  } else {
    // resolve CLI patterns based on the current working directory
    to_absolute_globs(&args.file_patterns, cwd)
  });

  process_file_pattern_slashes(&mut file_patterns);
  return file_patterns;
}

fn get_exclude_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(
    if args.exclude_file_patterns.is_empty() {
      config.excludes.clone()
    } else {
      // resolve CLI patterns based on the current working directory
      to_absolute_globs(&args.exclude_file_patterns, cwd)
    }
    .into_iter()
    .map(|exclude| if exclude.starts_with("!") { exclude } else { format!("!{}", exclude) }),
  );

  if !args.allow_node_modules {
    // glob walker will not search the children of a directory once it's ignored like this
    let node_modules_exclude = String::from("!**/node_modules");
    if !file_patterns.contains(&node_modules_exclude) {
      file_patterns.push(node_modules_exclude);
    }
  }
  process_file_pattern_slashes(&mut file_patterns);
  return file_patterns;
}

fn process_file_pattern_slashes(file_patterns: &mut Vec<String>) {
  for file_pattern in file_patterns.iter_mut() {
    // Convert all backslashes to forward slashes.
    // It is true that this means someone cannot specify patterns that
    // match files with backslashes in their name on Linux, however,
    // it is more desirable for this CLI to work the same way no matter
    // what operation system the user is on and for the CLI to match
    // backslashes as a path separator.
    *file_pattern = file_pattern.replace("\\", "/");

    // glob walker doesn't support having `./` at the front of paths, so just remove them when they appear
    if file_pattern.starts_with("./") {
      *file_pattern = String::from(&file_pattern[2..]);
    }
    if file_pattern.starts_with("!./") {
      *file_pattern = format!("!{}", &file_pattern[3..]);
    }
  }
}
