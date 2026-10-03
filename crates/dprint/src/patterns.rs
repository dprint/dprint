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
use crate::utils::expand_braces;
use crate::utils::is_absolute_pattern;
use crate::utils::is_negated_glob;
use crate::utils::non_negated_glob;
use crate::utils::resolve_global_gitignore_lines;
use crate::utils::rewrite_literal_arg_pattern;
use crate::utils::rewrite_literal_arg_patterns;

pub struct FileMatcherOptions<'a> {
  pub config: &'a ResolvedConfig,
  pub args: &'a FilePatternArgs,
  pub root_dir: &'a CanonicalizedPathBuf,
  /// An explicitly specified file path (ex. the `--stdin` path) that should
  /// override what's in the gitignore the same way an explicit fmt arg does.
  pub specified_file_path: Option<&'a Path>,
  /// Whether to check if a gitignore file was changed, added or removed since
  /// it was read each time a file is matched. This is for a matcher that's
  /// kept around (ex. in the language server) rather than used for a single run.
  pub detect_gitignore_changes: bool,
}

pub struct FileMatcher<TEnvironment: Environment> {
  glob_matcher: GlobMatcher,
  gitignores: Option<GitIgnoreTree<TEnvironment>>,
}

impl<TEnvironment: Environment> FileMatcher<TEnvironment> {
  pub fn new(environment: TEnvironment, opts: FileMatcherOptions) -> Result<Self> {
    let FileMatcherOptions {
      config,
      args,
      root_dir,
      specified_file_path,
      detect_gitignore_changes,
    } = opts;
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
          detect_changes: detect_gitignore_changes,
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
    if matches!(match_result, GlobMatchesDetail::Excluded | GlobMatchesDetail::NotMatched) {
      return false;
    }
    // done once up front because the checks below resolve the gitignores
    // of the same ancestor directories for the file and each ancestor
    if let Some(gitignores) = self.gitignores.as_mut() {
      gitignores.refresh_for_file(file_path);
    }
    if matches!(match_result, GlobMatchesDetail::Matched) && self.is_gitignored(file_path, /* is dir */ false) {
      return false;
    }
    // ensure the parents aren't ignored (skipping the file itself, which was
    // checked above with file semantics instead of dir semantics, and stopping
    // at the base directory, which a traversal starts within rather than
    // descends into)
    if !file_path.starts_with(self.glob_matcher.base_dir()) {
      return false;
    }
    for ancestor in file_path.ancestors().skip(1) {
      if ancestor == self.glob_matcher.base_dir().as_ref() {
        break;
      }
      if let Ok(path) = ancestor.strip_prefix(self.glob_matcher.base_dir()) {
        match self.glob_matcher.check_exclude(path, true) {
          ExcludeMatchDetail::Excluded => return false,
          ExcludeMatchDetail::OptedOutExclude => {}
          ExcludeMatchDetail::NotExcluded => {
            // the gitignore tree resolves gitignore files by walking the
            // path's ancestor directories, so pass the absolute path
            if self.is_gitignored(ancestor, /* is dir */ true) {
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

/// Matches paths against a list of patterns evaluated in order, like `includes`
/// and `excludes`, where the last matching pattern wins. This is used for plugin
/// `associations` and the `files` of plugin configuration `overrides`.
pub struct OrderedPatternsMatcher {
  matcher: GlobMatcher,
}

impl OrderedPatternsMatcher {
  pub fn new(patterns: &[String], config_base_path: &CanonicalizedPathBuf) -> Result<Self> {
    let matcher = GlobMatcher::new(
      GlobPatterns {
        shebangs: Vec::new(),
        arg_includes: None,
        config_includes: None,
        arg_excludes: None,
        // use an order aware exclude matcher with the patterns inverted, so a
        // matching pattern opts the path out of being excluded and a matching
        // negated pattern excludes it
        config_excludes: process_config_patterns(patterns)
          .map(|pattern| new_config_glob_pattern(pattern, config_base_path).invert())
          .collect(),
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: config_base_path.clone(),
      },
    )?;
    Ok(Self { matcher })
  }

  /// Gets if the last pattern that matches the path isn't negated.
  pub fn matches(&self, path: impl AsRef<Path>) -> bool {
    self.matcher.exclude_detail(path) == ExcludeMatchDetail::OptedOutExclude
  }

  /// Gets if the last pattern that matches the path is negated.
  pub fn is_excluded(&self, path: impl AsRef<Path>) -> bool {
    self.matcher.exclude_detail(path) == ExcludeMatchDetail::Excluded
  }
}

pub fn get_all_file_patterns(config: &ResolvedConfig, args: &FilePatternArgs, cwd: &CanonicalizedPathBuf, environment: &impl Environment) -> GlobPatterns {
  GlobPatterns {
    config_includes: get_config_includes_file_patterns(config, args, cwd, environment),
    // resolve CLI patterns based on the current working directory
    arg_includes: args
      .include_patterns
      .as_ref()
      .map(|patterns| process_cli_patterns(patterns, cwd, environment).collect()),
    config_excludes: get_config_exclude_file_patterns(config, args, cwd, environment),
    arg_excludes: if args.exclude_patterns.is_empty() {
      None
    } else {
      // resolve CLI patterns based on the current working directory
      Some(process_cli_patterns(&args.exclude_patterns, cwd, environment).collect())
    },
    // Shebang scripts are extensionless, so they can't be matched by the
    // includes up front. Discover them when shebang mappings are configured and
    // let plugin resolution filter them down to just the shebang matches. This
    // doesn't apply to an includes override since that should restrict the files.
    shebangs: match (&args.include_pattern_overrides, &config.shebangs) {
      (None, Some(shebangs)) => shebangs.keys().cloned().collect(),
      _ => Vec::new(),
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
      process_cli_override_patterns(includes_overrides, cwd, config, environment)
    }
    None => new_config_glob_patterns(process_config_patterns(config.includes.as_ref()?), &config.base_path),
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

  // `--allow-node-modules` is implicit when the cwd is within a `node_modules` directory:
  // changing into one is deliberate, so excluding everything there would leave nothing to
  // format no matter what the user asked for
  if !args.allow_node_modules && !is_in_node_modules(cwd.as_ref()) {
    // glob walker will not search the children of a directory once it's ignored like this
    //
    // A pattern only applies below its own base directory, so base it at both the cwd and
    // the config's base path: the cwd covers a config that lives above the cwd, while the
    // config's base path covers what the cwd can't reach (ex. an ancestor dir arg like
    // `dprint fmt ..` or a path outside the config's directory).
    //
    // These go before the other excludes because the last matching pattern wins, which
    // allows un-excluding a `node_modules` directory (ex. `!**/fixtures/node_modules`).
    let node_modules_exclude = String::from("**/node_modules");
    file_patterns.push(GlobPattern::new(node_modules_exclude.clone(), cwd.clone()));
    if config.base_path != *cwd {
      file_patterns.push(GlobPattern::new(node_modules_exclude, config.base_path.clone()));
    }
  }

  file_patterns.extend(match &args.exclude_pattern_overrides {
    Some(exclude_overrides) => {
      // resolve CLI patterns based on the current working directory
      process_cli_override_patterns(exclude_overrides, cwd, config, environment)
    }
    None => config
      .excludes
      .as_ref()
      .map(|excludes| new_config_glob_patterns(process_config_patterns(excludes), &config.base_path))
      .unwrap_or_default(),
  });

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

/// Whether the directory is a `node_modules` directory or is inside one.
fn is_in_node_modules(dir: &Path) -> bool {
  dir.components().any(|component| component.as_os_str() == "node_modules")
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

/// Processes the `--includes-override`/`--excludes-override` patterns, resolving
/// an existing literal name the same way normal CLI args are resolved (ex.
/// `--includes-override "routes/[id].svelte"` when that file exists).
fn process_cli_override_patterns(
  file_patterns: &[String],
  cwd: &CanonicalizedPathBuf,
  config: &ResolvedConfig,
  environment: &impl Environment,
) -> Vec<GlobPattern> {
  process_cli_patterns(file_patterns, cwd, environment)
    .map(|mut pattern| {
      rewrite_literal_arg_pattern(environment, &mut pattern, &config.base_path);
      pattern
    })
    .collect()
}

/// Processes CLI-provided patterns, expanding any brace groups that span
/// path components into separate patterns. A pattern naming an existing path
/// isn't expanded because it's matched literally (ex. a `{a,b}` directory).
fn process_cli_patterns<'a>(
  file_patterns: &'a [String],
  cwd: &'a CanonicalizedPathBuf,
  environment: &'a impl Environment,
) -> impl Iterator<Item = GlobPattern> + 'a {
  file_patterns
    .iter()
    .flat_map(move |pattern| {
      let pattern = process_file_pattern_slashes(pattern);
      let is_existing_path = pattern.contains('{') && environment.path_exists(cwd.join(non_negated_glob(&pattern)));
      let (literal, pattern) = if is_existing_path { (Some(pattern), None) } else { (None, Some(pattern)) };
      literal.into_iter().chain(pattern.into_iter().flat_map(expand_braces))
    })
    .map(move |pattern| process_cli_pattern(&pattern, cwd, environment))
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

/// Creates the glob patterns for a config file's processed patterns, which are
/// relative to the config file's directory. A pattern starting with `../` is
/// based at the corresponding ancestor directory.
pub fn new_config_glob_patterns(patterns: impl IntoIterator<Item = String>, config_base_path: &CanonicalizedPathBuf) -> Vec<GlobPattern> {
  patterns.into_iter().map(|pattern| new_config_glob_pattern(pattern, config_base_path)).collect()
}

pub fn new_config_glob_pattern(pattern: String, config_base_path: &CanonicalizedPathBuf) -> GlobPattern {
  let is_negated = is_negated_glob(&pattern);
  let non_negated = non_negated_glob(&pattern);
  let mut remaining = non_negated.strip_prefix("./").unwrap_or(non_negated);
  if !remaining.starts_with("../") {
    return GlobPattern::new(pattern, config_base_path.clone());
  }
  let mut base_dir = config_base_path.clone();
  while let Some(rest) = remaining.strip_prefix("../") {
    if let Some(parent) = base_dir.parent() {
      base_dir = parent;
    }
    remaining = rest;
  }
  // anchor the pattern to the ancestor directory
  let relative_pattern = format!("{}./{}", if is_negated { "!" } else { "" }, remaining);
  GlobPattern::new(relative_pattern, base_dir)
}

pub fn process_config_patterns(file_patterns: &[String]) -> impl Iterator<Item = String> + '_ {
  file_patterns.iter().flat_map(|p| process_config_pattern(p))
}

/// Processes a config file pattern, which may be multiple patterns when it
/// has a brace group that spans path components (ex. `{.,src/**}/*.js`).
pub fn process_config_pattern(file_pattern: &str) -> impl Iterator<Item = String> + use<> {
  expand_braces(process_file_pattern_slashes(file_pattern)).map(process_expanded_config_pattern)
}

fn process_expanded_config_pattern(mut file_pattern: String) -> String {
  // make config patterns that start with `/` be relative
  if file_pattern.starts_with('/') {
    file_pattern.insert(0, '.');
  } else if file_pattern.starts_with("!/") {
    file_pattern.insert(1, '.');
  }
  file_pattern
}

#[cfg(test)]
mod test {
  use std::path::PathBuf;

  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn should_get_if_in_node_modules() {
    assert!(is_in_node_modules(Path::new("/node_modules")));
    assert!(is_in_node_modules(Path::new("/node_modules/pkg")));
    assert!(is_in_node_modules(Path::new("/a/node_modules/pkg/sub")));
    assert!(!is_in_node_modules(Path::new("/")));
    assert!(!is_in_node_modules(Path::new("/a/sub")));
    // only a whole component counts
    assert!(!is_in_node_modules(Path::new("/a/my_node_modules/pkg")));
    assert!(!is_in_node_modules(Path::new("/a/node_modules_old")));
  }

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

  #[test]
  fn should_expand_brace_groups_in_cli_patterns() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/sub/dir{a").unwrap();
    environment.write_file("/sub/dir{a/b,c}.txt", "").unwrap();
    let cwd = CanonicalizedPathBuf::new_for_testing("/sub");
    let process = |pattern: &str| {
      process_cli_patterns(&[pattern.to_string()], &cwd, &environment)
        .map(|p| (p.relative_pattern, p.base_dir.to_string_lossy().replace('\\', "/")))
        .collect::<Vec<_>>()
    };
    let sub = |pattern: &str| (pattern.to_string(), "/sub".to_string());
    let root = |pattern: &str| (pattern.to_string(), "/".to_string());
    assert_eq!(process("{.,src/**}/*.js"), [sub("./*.js"), sub("./src/**/*.js")]);
    assert_eq!(process("!{.,src/**}/*.js"), [sub("!./*.js"), sub("!./src/**/*.js")]);
    assert_eq!(process("{../a,b}/*.js"), [root("./a/*.js"), sub("./b/*.js")]);
    // stays relative to the cwd
    assert_eq!(process("{,src/dir}/*.js"), [sub("./*.js"), sub("./src/dir/*.js")]);
    // an existing path is not expanded
    assert_eq!(process("dir{a/b,c}.txt"), [sub("./dir{a/b,c}.txt")]);
    assert_eq!(process("!dir{a/b,c}.txt"), [sub("!./dir{a/b,c}.txt")]);
    assert_eq!(process("other{a/b,c}.txt"), [sub("./othera/b.txt"), sub("./otherc.txt")]);
  }

  #[track_caller]
  fn assert_cli_pattern(file_pattern: &str, cwd: &str, expected_pattern: &str, expected_base_dir: &str) {
    let environment = TestEnvironment::new();
    let pattern = process_cli_pattern(file_pattern, &CanonicalizedPathBuf::new_for_testing(cwd), &environment);
    assert_eq!(pattern.relative_pattern, expected_pattern);
    assert_eq!(pattern.base_dir, CanonicalizedPathBuf::new_for_testing(expected_base_dir));
  }

  #[test]
  fn should_create_config_glob_pattern_relative_to_config_dir() {
    let base = CanonicalizedPathBuf::new_for_testing("/a/b");
    let pattern = |text: &str| {
      let pattern = new_config_glob_pattern(text.to_string(), &base);
      (pattern.relative_pattern, pattern.base_dir.to_string_lossy().replace('\\', "/"))
    };
    assert_eq!(pattern("src/**/*.ts"), ("src/**/*.ts".to_string(), "/a/b".to_string()));
    assert_eq!(pattern("./src"), ("./src".to_string(), "/a/b".to_string()));
    assert_eq!(pattern("../src/**"), ("./src/**".to_string(), "/a".to_string()));
    assert_eq!(pattern("./../src"), ("./src".to_string(), "/a".to_string()));
    assert_eq!(pattern("!../src"), ("!./src".to_string(), "/a".to_string()));
    assert_eq!(pattern("../../**/Cargo.toml"), ("./**/Cargo.toml".to_string(), "/".to_string()));
    // stops at the root directory
    assert_eq!(pattern("../../../src"), ("./src".to_string(), "/".to_string()));
  }

  #[test]
  fn should_process_config_pattern() {
    let process = |pattern: &str| process_config_pattern(pattern).collect::<Vec<_>>();
    assert_eq!(process("/test"), ["./test"]);
    assert_eq!(process("./test"), ["./test"]);
    assert_eq!(process("test"), ["test"]);
    assert_eq!(process("**/test"), ["**/test"]);

    assert_eq!(process("!/test"), ["!./test"]);
    assert_eq!(process("!./test"), ["!./test"]);
    assert_eq!(process("!test"), ["!test"]);
    assert_eq!(process("!**/test"), ["!**/test"]);

    // brace groups spanning path components
    assert_eq!(process("{/test,sub/**}/*.js"), ["./test/*.js", "sub/**/*.js"]);
    assert_eq!(process("!{/test,sub/**}/*.js"), ["!./test/*.js", "!sub/**/*.js"]);
  }

  #[test]
  fn should_match_brace_groups_spanning_path_components() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let new_matcher = |includes: &[&str], excludes: &[&str]| {
      let to_patterns = |patterns: &[&str]| {
        let patterns = patterns.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        new_config_glob_patterns(process_config_patterns(&patterns), &cwd)
      };
      GlobMatcher::new(
        GlobPatterns {
          shebangs: Vec::new(),
          arg_includes: None,
          config_includes: Some(to_patterns(includes)),
          arg_excludes: None,
          config_excludes: to_patterns(excludes),
        },
        &GlobMatcherOptions {
          case_sensitive: true,
          base_dir: cwd.clone(),
        },
      )
      .unwrap()
    };

    let matcher = new_matcher(&["{.,src/**,worker}/*.js"], &[]);
    assert_eq!(matcher.matches_detail("/testing/dir/src/foo/match.js"), GlobMatchesDetail::Matched);
    assert_eq!(matcher.matches_detail("/testing/dir/src/match.js"), GlobMatchesDetail::Matched);
    assert_eq!(matcher.matches_detail("/testing/dir/match.js"), GlobMatchesDetail::Matched);
    assert_eq!(matcher.matches_detail("/testing/dir/worker/match.js"), GlobMatchesDetail::Matched);
    assert_eq!(matcher.matches_detail("/testing/dir/foo/not_match.js"), GlobMatchesDetail::NotMatched);
    assert_eq!(matcher.matches_detail("/testing/dir/worker/foo/not_match.js"), GlobMatchesDetail::NotMatched);

    let matcher = new_matcher(&["**/*.js"], &["{.,src/**}/*.js"]);
    assert_eq!(matcher.matches_detail("/testing/dir/match.js"), GlobMatchesDetail::Excluded);
    assert_eq!(matcher.matches_detail("/testing/dir/src/match.js"), GlobMatchesDetail::Excluded);
    assert_eq!(matcher.matches_detail("/testing/dir/src/foo/match.js"), GlobMatchesDetail::Excluded);
    assert_eq!(matcher.matches_detail("/testing/dir/worker/match.js"), GlobMatchesDetail::Matched);
  }

  #[test]
  fn handles_ignored_dir() {
    let environment = TestEnvironment::new();
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        shebangs: Vec::new(),
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
        shebangs: Vec::new(),
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

  #[test]
  fn handles_gitignored_ancestor_dir() {
    let environment = TestEnvironment::new();
    // note the cwd differs from the base dir, so gitignore resolution
    // must not depend on relative paths
    let base_dir = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    environment.mk_dir_all(base_dir.as_ref()).unwrap();
    environment.write_file("/testing/dir/.gitignore", "ignored-dir/\nsub.ts/\n").unwrap();
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        shebangs: Vec::new(),
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), base_dir.clone())]),
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: base_dir.clone(),
      },
    )
    .unwrap();
    let mut file_matcher = FileMatcher {
      glob_matcher,
      gitignores: Some(GitIgnoreTree::new(environment, GitIgnoreTreeOptions::default())),
    };
    // a file within a gitignored ancestor dir doesn't match
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/ignored-dir/no-match.ts", false);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/ignored-dir/nested/no-match.ts", false);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/other/match.ts", true);
    // a directory-only gitignore pattern (`sub.ts/`) doesn't apply to a
    // file with that name
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/sub.ts", true);
  }

  #[test]
  fn ignores_gitignored_base_dir_itself() {
    let environment = TestEnvironment::new();
    let base_dir = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    environment.mk_dir_all(base_dir.as_ref()).unwrap();
    // the base dir is gitignored by its parent
    environment.write_file("/testing/.gitignore", "dir/\n").unwrap();
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        shebangs: Vec::new(),
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), base_dir.clone())]),
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: base_dir.clone(),
      },
    )
    .unwrap();
    let mut file_matcher = FileMatcher {
      glob_matcher,
      gitignores: Some(GitIgnoreTree::new(environment, GitIgnoreTreeOptions::default())),
    };
    // a traversal starts within the base dir rather than descending into it,
    // so the base dir being gitignored doesn't exclude everything
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/match.ts", true);
    assert_matches_dir_and_not_ignored(&mut file_matcher, "/testing/dir/sub/match.ts", true);
  }

  #[track_caller]
  fn assert_matches_dir_and_not_ignored(matcher: &mut FileMatcher<TestEnvironment>, path: &str, expected: bool) {
    assert_eq!(matcher.matches_and_dir_not_ignored(&PathBuf::from(path)), expected);
  }
}
