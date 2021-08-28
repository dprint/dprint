use std::path::Path;

use dprint_cli_core::types::ErrBox;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::environment::Environment;
use crate::utils::{to_absolute_glob, to_absolute_globs};

use super::configuration::ResolvedConfig;
use super::CliArgs;

pub struct FileMatcher {
  patterns_globset: GlobSet,
}

impl FileMatcher {
  pub fn new(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Self, ErrBox> {
    let cwd = environment.cwd();
    let cwd_str = cwd.to_string_lossy();
    let patterns = get_all_file_patterns(config, args, &cwd_str);
    let patterns_globset = build_glob_set(&patterns)?;

    Ok(FileMatcher { patterns_globset })
  }

  pub fn matches(&self, file_path: &Path) -> bool {
    let mut file_path = file_path.to_string_lossy().to_string();
    process_file_pattern_slashses(&mut file_path);
    // issue on windows where V:/ was not matching for pattern with v:/
    self.patterns_globset.is_match(&file_path.to_lowercase())
  }
}

fn build_glob_set(file_patterns: &Vec<String>) -> Result<GlobSet, ErrBox> {
  let mut builder = GlobSetBuilder::new();
  for pattern in file_patterns {
    builder.add(Glob::new(&pattern.to_lowercase())?);
  }
  return Ok(builder.build().unwrap());
}

pub fn get_all_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = get_include_file_patterns(config, args, cwd);
  file_patterns.append(&mut get_exclude_file_patterns(config, args, cwd));
  process_file_patterns_slashes(&mut file_patterns);
  return file_patterns;
}

fn get_include_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(if args.file_patterns.is_empty() {
    to_absolute_globs(&config.includes, &config.base_path.to_string_lossy())
  } else {
    // resolve CLI patterns based on the current working directory
    to_absolute_globs(&args.file_patterns, cwd)
  });

  return file_patterns;
}

fn get_exclude_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(
    if args.exclude_file_patterns.is_empty() {
      to_absolute_globs(&config.excludes, &config.base_path.to_string_lossy())
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
    let exclude_node_module_patterns = vec![
      to_absolute_glob(&node_modules_exclude, cwd),
      to_absolute_glob(&node_modules_exclude, &config.base_path.to_string_lossy()),
    ];
    for node_modules_exclude in exclude_node_module_patterns {
      if !file_patterns.contains(&node_modules_exclude) {
        file_patterns.push(node_modules_exclude);
      }
    }
  }
  return file_patterns;
}

fn process_file_patterns_slashes(file_patterns: &mut Vec<String>) {
  for file_pattern in file_patterns.iter_mut() {
    process_file_pattern_slashses(file_pattern);
  }
}

fn process_file_pattern_slashses(file_pattern: &mut String) {
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
