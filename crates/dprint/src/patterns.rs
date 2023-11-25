use std::path::Path;

use anyhow::Result;

use crate::arg_parser::FilePatternArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::is_absolute_pattern;
use crate::utils::is_negated_glob;
use crate::utils::GlobMatcher;
use crate::utils::GlobMatcherOptions;
use crate::utils::GlobPattern;
use crate::utils::GlobPatterns;

pub struct FileMatcher {
  glob_matcher: GlobMatcher,
}

impl FileMatcher {
  pub fn new(config: &ResolvedConfig, args: &FilePatternArgs, environment: &impl Environment) -> Result<Self> {
    let cwd = environment.cwd();
    let patterns = get_all_file_patterns(config, args, &cwd);
    let glob_matcher = GlobMatcher::new(
      patterns,
      &GlobMatcherOptions {
        case_sensitive: !cfg!(windows),
        base_dir: config.base_path.clone(),
      },
    )?;

    Ok(FileMatcher { glob_matcher })
  }

  pub fn matches(&self, file_path: impl AsRef<Path>) -> bool {
    self.glob_matcher.matches(&file_path)
  }

  /// More expensive check for if the directory is already ignored.
  /// Prefer using `matches` if you already know the parent directory
  /// isn't ignored.
  pub fn matches_and_dir_not_ignored(&self, file_path: impl AsRef<Path>) -> bool {
    self.glob_matcher.matches_and_dir_not_ignored(file_path)
  }
}

pub fn get_patterns_as_glob_matcher(patterns: &[String], config_base_path: &CanonicalizedPathBuf) -> Result<GlobMatcher> {
  let patterns = process_config_patterns(patterns);
  let (includes, excludes) = patterns.into_iter().partition(|p| !is_negated_glob(p));
  GlobMatcher::new(
    GlobPatterns {
      includes: Some(GlobPattern::new_vec(includes, config_base_path.clone())),
      excludes: GlobPattern::new_vec(excludes, config_base_path.clone()),
    },
    &GlobMatcherOptions {
      case_sensitive: !cfg!(windows),
      base_dir: config_base_path.clone(),
    },
  )
}

pub fn get_all_file_patterns(config: &ResolvedConfig, args: &FilePatternArgs, cwd: &CanonicalizedPathBuf) -> GlobPatterns {
  GlobPatterns {
    includes: get_include_file_patterns(config, args, cwd),
    excludes: get_exclude_file_patterns(config, args, cwd),
  }
}

fn get_include_file_patterns(config: &ResolvedConfig, args: &FilePatternArgs, cwd: &CanonicalizedPathBuf) -> Option<Vec<GlobPattern>> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(if args.file_patterns.is_empty() {
    GlobPattern::new_vec(process_config_patterns(config.includes.as_ref()?).collect(), config.base_path.clone())
  } else {
    // resolve CLI patterns based on the current working directory
    GlobPattern::new_vec(args.file_patterns.iter().map(|p| process_cli_pattern(p, cwd)).collect(), cwd.clone())
  });

  Some(file_patterns)
}

fn get_exclude_file_patterns(config: &ResolvedConfig, args: &FilePatternArgs, cwd: &CanonicalizedPathBuf) -> Vec<GlobPattern> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(
    if args.exclude_file_patterns.is_empty() {
      if let Some(excludes) = &config.excludes {
        GlobPattern::new_vec(process_config_patterns(excludes).collect(), config.base_path.clone())
      } else {
        Vec::new()
      }
    } else {
      // resolve CLI patterns based on the current working directory
      GlobPattern::new_vec(args.exclude_file_patterns.iter().map(|p| process_cli_pattern(p, cwd)).collect(), cwd.clone())
    }
    .into_iter()
    .map(|pattern| pattern.into_negated()),
  );

  if !args.allow_node_modules {
    // glob walker will not search the children of a directory once it's ignored like this
    let node_modules_exclude = String::from("!**/node_modules");
    let exclude_node_module_patterns = vec![
      GlobPattern::new(node_modules_exclude.clone(), cwd.clone()),
      GlobPattern::new(node_modules_exclude, config.base_path.clone()),
    ];
    for node_modules_exclude in exclude_node_module_patterns {
      if !file_patterns.contains(&node_modules_exclude) {
        file_patterns.push(node_modules_exclude);
      }
    }
  }
  file_patterns
}

fn process_file_pattern_slashes(file_pattern: &str) -> String {
  // Convert all backslashes to forward slashes.
  // It is true that this means someone cannot specify patterns that
  // match files with backslashes in their name on Linux, however,
  // it is more desirable for this CLI to work the same way no matter
  // what operation system the user is on and for the CLI to match
  // backslashes as a path separator.
  file_pattern.replace('\\', "/")
}

fn process_cli_pattern(file_pattern: &str, cwd: &CanonicalizedPathBuf) -> String {
  let file_pattern = process_file_pattern_slashes(file_pattern);
  if is_absolute_pattern(&file_pattern) {
    let is_negated = is_negated_glob(&file_pattern);
    let cwd = process_file_pattern_slashes(&cwd.to_string_lossy());
    let file_pattern = if is_negated { &file_pattern[1..] } else { &file_pattern };
    format!(
      "{}./{}",
      if is_negated { "!" } else { "" },
      if file_pattern.starts_with(&cwd) {
        file_pattern[cwd.len()..].trim_start_matches('/')
      } else {
        file_pattern
      },
    )
  } else if file_pattern.starts_with("./") || file_pattern.starts_with("!./") {
    file_pattern
  } else {
    // make all cli specified patterns relative
    if is_negated_glob(&file_pattern) {
      format!("!./{}", &file_pattern[1..])
    } else {
      format!("./{}", file_pattern)
    }
  }
}

pub fn process_config_patterns(file_patterns: &[String]) -> impl Iterator<Item = String> + '_ {
  file_patterns.iter().map(|p| process_config_pattern(p))
}

fn process_config_pattern(file_pattern: &str) -> String {
  let file_pattern = process_file_pattern_slashes(file_pattern);
  // make config patterns that start with `/` be relative
  if file_pattern.starts_with('/') {
    format!(".{}", file_pattern)
  } else if file_pattern.starts_with("!/") {
    format!("!.{}", &file_pattern[1..])
  } else {
    file_pattern
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_process_cli_patterns() {
    assert_eq!(do_process_cli_pattern("/test", "/"), "./test");
    assert_eq!(do_process_cli_pattern("./test", "/"), "./test");
    assert_eq!(do_process_cli_pattern("test", "/"), "./test");
    assert_eq!(do_process_cli_pattern("**/test", "/"), "./**/test");

    assert_eq!(do_process_cli_pattern("!/test", "/"), "!./test");
    assert_eq!(do_process_cli_pattern("!./test", "/"), "!./test");
    assert_eq!(do_process_cli_pattern("!test", "/"), "!./test");
    assert_eq!(do_process_cli_pattern("!**/test", "/"), "!./**/test");
  }

  #[cfg(windows)]
  #[test]
  fn should_process_cli_patterns_windows() {
    assert_eq!(do_process_cli_pattern("C:/test", "C:\\"), "./test");
    assert_eq!(do_process_cli_pattern("C:/test/other", "C:\\test\\"), "./other");
    assert_eq!(do_process_cli_pattern("C:/test/other", "C:\\test"), "./other");

    assert_eq!(do_process_cli_pattern("!C:/test", "C:\\"), "!./test");
    assert_eq!(do_process_cli_pattern("!C:/test/other", "C:\\test\\"), "!./other");
  }

  fn do_process_cli_pattern(file_pattern: &str, cwd: &str) -> String {
    process_cli_pattern(file_pattern, &CanonicalizedPathBuf::new_for_testing(cwd))
  }

  #[test]
  fn should_process_config_pattern() {
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
