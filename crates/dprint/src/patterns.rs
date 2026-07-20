use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;

use crate::arg_parser::FilePatternArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::ExcludeMatchDetail;
use crate::utils::GitIgnoreTree;
use crate::utils::GitIgnoreTreeOptions;
use crate::utils::GlobMatcher;
use crate::utils::GlobMatcherOptions;
use crate::utils::GlobMatchesDetail;
use crate::utils::GlobPattern;
use crate::utils::GlobPatterns;
use crate::utils::is_absolute_pattern;
use crate::utils::is_negated_glob;
use crate::utils::non_negated_glob;
use crate::utils::resolve_global_gitignore_lines;
use crate::utils::rewrite_literal_arg_pattern;
use crate::utils::rewrite_literal_arg_patterns;

pub struct FileMatcher<TEnvironment: Environment> {
  glob_matcher: GlobMatcher,
  gitignores: Option<GitIgnoreTree<TEnvironment>>,
}

impl<TEnvironment: Environment> FileMatcher<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    config: &ResolvedConfig,
    args: &FilePatternArgs,
    root_dir: &CanonicalizedPathBuf,
    specified_file_path: Option<&Path>,
  ) -> Result<Self> {
    let mut patterns = get_all_file_patterns(config, args, root_dir, &environment);
    // resolve args with an existing literal name the same way `glob()` does
    // (ex. `--stdin` matching must agree with a normal `fmt`)
    rewrite_literal_arg_patterns(&environment, &mut patterns, &config.base_path);
    let gitignores = if args.no_gitignore {
      None
    } else {
      let global_gitignore_lines = resolve_global_gitignore_lines(&environment);
      // explicitly specified paths should override what's in the gitignore
      let mut include_paths = patterns.include_paths();
      if let Some(path) = specified_file_path {
        include_paths.push(path.to_path_buf());
      }
      Some(GitIgnoreTree::new(
        environment,
        GitIgnoreTreeOptions {
          include_paths,
          global_gitignore_lines,
        },
      ))
    };
    let glob_matcher = GlobMatcher::new(
      patterns,
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: config.base_path.clone(),
      },
    )?;

    Ok(FileMatcher { glob_matcher, gitignores })
  }

  /// Gets whether the file matches, also checking that none of its
  /// ancestor directories are excluded or gitignored so exclusions apply
  /// the same way they do during a directory traversal.
  pub fn matches_and_dir_not_ignored(&mut self, file_path: &Path) -> bool {
    let match_result = self.glob_matcher.matches_detail(file_path);
    match match_result {
      GlobMatchesDetail::Matched => {
        if self.is_gitignored(file_path, /* is dir */ false) {
          return false;
        }
      }
      GlobMatchesDetail::MatchedOptedOutExclude => {}
      GlobMatchesDetail::Excluded | GlobMatchesDetail::NotMatched => return false,
    };
    // ensure the parents aren't ignored
    if !file_path.starts_with(self.glob_matcher.base_dir()) {
      return false;
    }
    for ancestor in file_path.ancestors() {
      if let Ok(path) = ancestor.strip_prefix(self.glob_matcher.base_dir()) {
        match self.glob_matcher.check_exclude(path, true) {
          ExcludeMatchDetail::Excluded => return false,
          ExcludeMatchDetail::OptedOutExclude => {}
          ExcludeMatchDetail::NotExcluded => {
            if self.is_gitignored(path, /* is dir */ true) {
              return false;
            }
          }
        }
      } else {
        break;
      }
    }
    true
  }

  fn is_gitignored(&mut self, path: &Path, is_dir: bool) -> bool {
    let Some(gitignores) = self.gitignores.as_mut() else {
      return false;
    };
    let Some(gitignore) = gitignores.get_resolved_git_ignore_for_file(path) else {
      return false;
    };
    gitignore.is_ignored(path, is_dir)
  }
}

pub fn get_patterns_as_glob_matcher(patterns: &[String], config_base_path: &CanonicalizedPathBuf) -> Result<GlobMatcher> {
  let patterns = process_config_patterns(patterns);
  let (includes, excludes) = patterns.into_iter().partition(|p| !is_negated_glob(p));
  GlobMatcher::new(
    GlobPatterns {
      arg_includes: None,
      config_includes: Some(GlobPattern::new_vec(includes, config_base_path.clone())),
      arg_excludes: None,
      config_excludes: excludes
        .into_iter()
        .map(|relative_pattern| GlobPattern::new(relative_pattern, config_base_path.clone()).invert())
        .collect(),
    },
    &GlobMatcherOptions {
      case_sensitive: true,
      base_dir: config_base_path.clone(),
    },
  )
}

pub fn get_all_file_patterns(config: &ResolvedConfig, args: &FilePatternArgs, cwd: &CanonicalizedPathBuf, environment: &impl Environment) -> GlobPatterns {
  GlobPatterns {
    config_includes: get_config_includes_file_patterns(config, args, cwd, environment),
    arg_includes: if args.include_patterns.is_empty() {
      None
    } else {
      // resolve CLI patterns based on the current working directory
      Some(args.include_patterns.iter().map(|p| process_cli_pattern(p, cwd, environment)).collect())
    },
    config_excludes: get_config_exclude_file_patterns(config, args, cwd, environment),
    arg_excludes: if args.exclude_patterns.is_empty() {
      None
    } else {
      // resolve CLI patterns based on the current working directory
      Some(args.exclude_patterns.iter().map(|p| process_cli_pattern(p, cwd, environment)).collect())
    },
  }
}

fn get_config_includes_file_patterns(
  config: &ResolvedConfig,
  args: &FilePatternArgs,
  cwd: &CanonicalizedPathBuf,
  environment: &impl Environment,
) -> Option<Vec<GlobPattern>> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(match &args.include_pattern_overrides {
    Some(includes_overrides) => {
      // resolve CLI patterns based on the current working directory
      includes_overrides
        .iter()
        .map(|p| process_cli_override_pattern(p, cwd, config, environment))
        .collect()
    }
    None => GlobPattern::new_vec(process_config_patterns(config.includes.as_ref()?).collect(), config.base_path.clone()),
  });

  Some(file_patterns)
}

fn get_config_exclude_file_patterns(
  config: &ResolvedConfig,
  args: &FilePatternArgs,
  cwd: &CanonicalizedPathBuf,
  environment: &impl Environment,
) -> Vec<GlobPattern> {
  let mut file_patterns = Vec::new();

  file_patterns.extend(match &args.exclude_pattern_overrides {
    Some(exclude_overrides) => {
      // resolve CLI patterns based on the current working directory
      exclude_overrides
        .iter()
        .map(|p| process_cli_override_pattern(p, cwd, config, environment))
        .collect::<Vec<_>>()
    }
    None => config
      .excludes
      .as_ref()
      .map(|excludes| GlobPattern::new_vec(process_config_patterns(excludes).collect(), config.base_path.clone()))
      .unwrap_or_default(),
  });

  // todo(THIS PR): document removing this flag in favour of a !**/node_modules pattern
  // and make this work with that
  if !args.allow_node_modules {
    // glob walker will not search the children of a directory once it's ignored like this
    let node_modules_exclude = String::from("**/node_modules");
    let mut exclude_node_module_patterns = vec![GlobPattern::new(node_modules_exclude.clone(), cwd.clone())];
    if !cwd.starts_with(&config.base_path) {
      exclude_node_module_patterns.push(GlobPattern::new(node_modules_exclude, config.base_path.clone()));
    }
    for node_modules_exclude in exclude_node_module_patterns {
      if !file_patterns.contains(&node_modules_exclude) {
        file_patterns.push(node_modules_exclude);
      }
    }
  }

  file_patterns
}

/// Processes CLI-provided file paths (ex. git staged files) the same way
/// as CLI patterns so they resolve to a base directory that contains them.
pub fn process_cli_path_args(paths: &[PathBuf], cwd: &CanonicalizedPathBuf, environment: &impl Environment) -> Vec<GlobPattern> {
  paths
    .iter()
    .map(|path| process_cli_pattern(&path.to_string_lossy(), cwd, environment))
    .collect()
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

/// Processes an `--includes-override`/`--excludes-override` pattern, resolving
/// an existing literal name the same way normal CLI args are resolved (ex.
/// `--includes-override "routes/[id].svelte"` when that file exists).
fn process_cli_override_pattern(file_pattern: &str, cwd: &CanonicalizedPathBuf, config: &ResolvedConfig, environment: &impl Environment) -> GlobPattern {
  let mut pattern = process_cli_pattern(file_pattern, cwd, environment);
  rewrite_literal_arg_pattern(environment, &mut pattern, &config.base_path);
  pattern
}

fn process_cli_pattern(file_pattern: &str, cwd: &CanonicalizedPathBuf, environment: &impl Environment) -> GlobPattern {
  let file_pattern = process_file_pattern_slashes(file_pattern);
  let is_negated = is_negated_glob(&file_pattern);
  let pattern = non_negated_glob(&file_pattern);
  if pattern == "." {
    return GlobPattern::new(if is_negated { "!./." } else { "**" }.to_string(), cwd.clone());
  }

  let absolute_pattern = normalize_path(if is_absolute_pattern(&file_pattern) {
    PathBuf::from(pattern)
  } else {
    cwd.join(pattern)
  });

  // resolve the pattern against the nearest ancestor directory it's within
  // so that patterns like ../file.txt or absolute paths outside the current
  // working directory get a base directory that contains them
  let mut base_dir = cwd.clone();
  loop {
    if let Ok(relative_pattern) = absolute_pattern.strip_prefix(base_dir.as_ref()) {
      return build_cli_pattern(relative_pattern, is_negated, base_dir);
    }

    let Some(parent) = base_dir.parent() else {
      break;
    };
    base_dir = parent;
  }

  // the pattern is on a different root than the cwd (ex. another drive
  // on Windows), so resolve it against its own root directory
  if let Some(root_dir) = absolute_pattern.ancestors().last().filter(|p| !p.as_os_str().is_empty())
    && let Ok(relative_pattern) = absolute_pattern.strip_prefix(root_dir)
    && let Ok(root_dir) = environment.canonicalize(root_dir)
  {
    return build_cli_pattern(relative_pattern, is_negated, root_dir);
  }

  GlobPattern::new(file_pattern, cwd.clone())
}

fn build_cli_pattern(relative_pattern: &Path, is_negated: bool, base_dir: CanonicalizedPathBuf) -> GlobPattern {
  let relative_pattern = process_file_pattern_slashes(&relative_pattern.to_string_lossy());
  let relative_pattern = format!("{}./{}", if is_negated { "!" } else { "" }, relative_pattern);
  GlobPattern::new(relative_pattern, base_dir)
}

fn normalize_path(path: PathBuf) -> PathBuf {
  let mut result = PathBuf::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => {
        result.pop();
      }
      component => result.push(component.as_os_str()),
    }
  }
  result
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
  use std::path::PathBuf;

  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn should_process_cli_patterns() {
    assert_cli_pattern("/test", "/", "./test", "/");
    assert_cli_pattern("./test", "/", "./test", "/");
    assert_cli_pattern("test", "/", "./test", "/");
    assert_cli_pattern("**/test", "/", "./**/test", "/");

    assert_cli_pattern("!/test", "/", "!./test", "/");
    assert_cli_pattern("!./test", "/", "!./test", "/");
    assert_cli_pattern("!test", "/", "!./test", "/");
    assert_cli_pattern("!**/test", "/", "!./**/test", "/");
    assert_cli_pattern("!.", "/", "!./.", "/");
    assert_cli_pattern("../test", "/sub", "./test", "/");
    assert_cli_pattern("/test", "/sub", "./test", "/");
  }

  #[cfg(windows)]
  #[test]
  fn should_process_cli_patterns_windows() {
    assert_cli_pattern("C:/test", "C:\\", "./test", "C:\\");
    assert_cli_pattern("C:/test/other", "C:\\test\\", "./other", "C:\\test\\");
    assert_cli_pattern("C:/test/other", "C:\\test", "./other", "C:\\test");
    assert_cli_pattern("../test", "C:\\sub", "./test", "C:\\");

    // a path on a different drive resolves against its own root
    {
      let environment = TestEnvironment::new();
      let pattern = process_cli_pattern("V:/test/file.txt", &CanonicalizedPathBuf::new_for_testing("C:\\sub"), &environment);
      assert_eq!(pattern.relative_pattern, "./test/file.txt");
      assert_eq!(pattern.base_dir, environment.canonicalize("V:/").unwrap());
    }

    assert_cli_pattern("!C:/test", "C:\\", "!./test", "C:\\");
    assert_cli_pattern("!C:/test/other", "C:\\test\\", "!./other", "C:\\test\\");
  }

  #[track_caller]
  fn assert_cli_pattern(file_pattern: &str, cwd: &str, expected_pattern: &str, expected_base_dir: &str) {
    let environment = TestEnvironment::new();
    let pattern = process_cli_pattern(file_pattern, &CanonicalizedPathBuf::new_for_testing(cwd), &environment);
    assert_eq!(pattern.relative_pattern, expected_pattern);
    assert_eq!(pattern.base_dir, CanonicalizedPathBuf::new_for_testing(expected_base_dir));
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

  #[test]
  fn handles_ignored_dir() {
    let environment = TestEnvironment::new();
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("sub-dir".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    let mut file_matcher = FileMatcher {
      glob_matcher,
      gitignores: Some(GitIgnoreTree::new(environment, GitIgnoreTreeOptions::default())),
    };
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/match.ts", true);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/other/match.ts", true);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/sub-dir/no-match.ts", false);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/sub-dir/nested/no-match.ts", false);
  }

  #[test]
  fn handles_ignored_dir_while_include_is_sub_dir() {
    let environment = TestEnvironment::new();
    let base_dir = CanonicalizedPathBuf::new_for_testing("/");
    let cwd = CanonicalizedPathBuf::new_for_testing("/sub-dir");
    environment.mk_dir_all(&cwd).unwrap();
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        // notice cwd and base_dir are different. This will happen when the config
        // file is in an ancestor dir and the user has stepped into a folder
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("**/dist".to_string(), base_dir.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    let mut file_matcher = FileMatcher {
      glob_matcher,
      gitignores: Some(GitIgnoreTree::new(environment, GitIgnoreTreeOptions::default())),
    };
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/sub-dir/dir/match.ts", true);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/sub-dir/dir/other/match.ts", true);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/sub-dir/dist/no-match.ts", false);
  }

  #[track_caller]
  fn assert_matches_dir_and_not_ignored(matcher: &mut FileMatcher<TestEnvironment>, path: &str, expected: bool) {
    assert_eq!(matcher.matches_and_dir_not_ignored(&PathBuf::from(path)), expected);
  }
}
