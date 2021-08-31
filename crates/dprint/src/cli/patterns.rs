use std::path::Path;

use dprint_cli_core::types::ErrBox;

use crate::environment::Environment;
use crate::utils::{is_absolute_pattern, is_negated_glob, to_absolute_glob, to_absolute_globs, GlobMatcher, GlobMatcherOptions};

use super::configuration::ResolvedConfig;
use super::CliArgs;

pub struct FileMatcher {
  glob_matcher: GlobMatcher,
}

impl FileMatcher {
  pub fn new(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Self, ErrBox> {
    let cwd = environment.cwd();
    let cwd_str = cwd.to_string_lossy();
    let patterns = get_all_file_patterns(config, args, &cwd_str);
    let glob_matcher = GlobMatcher::new(
      &patterns,
      &GlobMatcherOptions {
        // issue on windows where V:/ was not matching for pattern with v:/
        case_insensitive: true,
      },
    )?;

    Ok(FileMatcher { glob_matcher })
  }

  pub fn matches(&self, file_path: &Path) -> bool {
    let mut file_path = file_path.to_string_lossy().to_string();
    process_file_pattern_slashes(&mut file_path);
    self.glob_matcher.is_match(&file_path)
  }
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
    to_absolute_globs(&process_config_patterns(&config.includes), &config.base_path.to_string_lossy())
  } else {
    // resolve CLI patterns based on the current working directory
    to_absolute_globs(&process_cli_patterns(&args.file_patterns), cwd)
  });

  return file_patterns;
}

fn get_exclude_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(
    if args.exclude_file_patterns.is_empty() {
      to_absolute_globs(&process_config_patterns(&config.excludes), &config.base_path.to_string_lossy())
    } else {
      // resolve CLI patterns based on the current working directory
      to_absolute_globs(&process_cli_patterns(&args.exclude_file_patterns), cwd)
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
    process_file_pattern_slashes(file_pattern);
  }
}

fn process_file_pattern_slashes(file_pattern: &mut String) {
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

fn process_cli_patterns(file_patterns: &Vec<String>) -> Vec<String> {
  file_patterns.iter().map(|pattern| process_cli_pattern(pattern)).collect()
}

fn process_cli_pattern(file_pattern: &str) -> String {
  if is_absolute_pattern(file_pattern) {
    file_pattern.to_string()
  } else if file_pattern.starts_with("./") || file_pattern.starts_with("!./") {
    file_pattern.to_string()
  } else {
    // make all cli specified patterns relative
    if is_negated_glob(file_pattern) {
      format!("!./{}", &file_pattern[1..])
    } else {
      format!("./{}", file_pattern)
    }
  }
}

fn process_config_patterns(file_patterns: &Vec<String>) -> Vec<String> {
  file_patterns.iter().map(|pattern| process_config_pattern(pattern)).collect()
}

fn process_config_pattern(file_pattern: &str) -> String {
  // make config patterns that start with `/` be relative
  if file_pattern.starts_with("/") {
    format!(".{}", file_pattern)
  } else if file_pattern.starts_with("!/") {
    format!("!.{}", &file_pattern[1..])
  } else {
    file_pattern.to_string()
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn it_should_process_cli_pattern() {
    assert_eq!(process_cli_pattern("/test"), "/test");
    assert_eq!(process_cli_pattern("C:\\test"), "C:\\test");
    assert_eq!(process_cli_pattern("./test"), "./test");
    assert_eq!(process_cli_pattern("test"), "./test");
    assert_eq!(process_cli_pattern("**/test"), "./**/test");

    assert_eq!(process_cli_pattern("!/test"), "!/test");
    assert_eq!(process_cli_pattern("!C:\\test"), "!C:\\test");
    assert_eq!(process_cli_pattern("!./test"), "!./test");
    assert_eq!(process_cli_pattern("!test"), "!./test");
    assert_eq!(process_cli_pattern("!**/test"), "!./**/test");
  }

  #[test]
  fn it_should_process_config_pattern() {
    assert_eq!(process_config_pattern("/test"), "./test");
    assert_eq!(process_config_pattern("./test"), "./test");
    assert_eq!(process_config_pattern("test"), "test");
    assert_eq!(process_config_pattern("**/test"), "**/test");

    assert_eq!(process_config_pattern("!/test"), "!./test");
    assert_eq!(process_config_pattern("!./test"), "!./test");
    assert_eq!(process_config_pattern("!test"), "!test");
    assert_eq!(process_config_pattern("!**/test"), "!**/test");
  }
}
