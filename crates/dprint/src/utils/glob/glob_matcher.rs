use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use ignore::Match;
use ignore::gitignore::Gitignore;
use ignore::gitignore::GitignoreBuilder;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;

use crate::environment::CanonicalizedPathBuf;

use super::GlobPattern;
use super::GlobPatterns;
use super::is_pattern;

pub struct GlobMatcherOptions {
  pub case_sensitive: bool,
  pub base_dir: CanonicalizedPathBuf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GlobMatchesDetail {
  /// Matched an includes pattern.
  Matched,
  /// Matched and opted out of gitignore exclusion.
  MatchedOptedOutExclude,
  /// Matched an excludes pattern.
  Excluded,
  /// Matched neither an includes or excludes pattern.
  NotMatched,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExcludeMatchDetail {
  Excluded,
  OptedOutExclude,
  NotExcluded,
}

pub struct GlobMatcher {
  base_dir: CanonicalizedPathBuf,
  config_include_matcher: Option<IncludeMatcher>,
  arg_include_matcher: Option<ArgIncludeMatcher>,
  config_exclude_matcher: ExcludeMatcher,
  arg_exclude_matcher: Option<ExcludeMatcher>,
}

impl GlobMatcher {
  pub fn new(patterns: GlobPatterns, opts: &GlobMatcherOptions) -> Result<GlobMatcher> {
    let base_dir = patterns
      .config_includes
      .as_ref()
      .and_then(|includes| get_base_dir(includes.iter().map(|p| &p.base_dir)))
      .unwrap_or_else(|| opts.base_dir.clone());

    // map the includes and excludes to have a new base
    let config_excludes = patterns
      .config_excludes
      .into_iter()
      .filter_map(|pattern| pattern.into_new_base(base_dir.clone()))
      .collect::<Vec<_>>();
    let config_includes = patterns.config_includes.map(|includes| {
      includes
        .into_iter()
        .filter_map(|pattern| pattern.into_new_base(base_dir.clone()))
        .collect::<Vec<_>>()
    });
    let arg_includes = patterns.arg_includes.map(|includes| {
      includes
        .into_iter()
        .filter_map(|pattern| pattern.into_new_base(base_dir.clone()))
        .collect::<Vec<_>>()
    });
    let arg_excludes = patterns.arg_excludes.map(|excludes| {
      excludes
        .into_iter()
        .filter_map(|pattern| pattern.into_new_base(base_dir.clone()))
        .collect::<Vec<_>>()
    });

    Ok(GlobMatcher {
      config_include_matcher: match config_includes {
        Some(includes) => Some(build_include_matcher(&includes, opts, &base_dir)?),
        None => None,
      },
      arg_include_matcher: match arg_includes {
        Some(includes) => Some(ArgIncludeMatcher {
          include: build_include_matcher(&includes, opts, &base_dir)?,
          // change the arg includes to go to the deepest base in order to make it
          // easier what directories should be ignored for traversal
          includes: includes.into_iter().map(|p| p.into_deepest_base()).collect(),
        }),
        None => None,
      },
      config_exclude_matcher: build_exclude_matcher(&config_excludes, opts, &base_dir)?,
      arg_exclude_matcher: match arg_excludes {
        Some(excludes) => Some(build_exclude_matcher(&excludes, opts, &base_dir)?),
        None => None,
      },
      base_dir,
    })
  }

  pub fn base_dir(&self) -> &CanonicalizedPathBuf {
    &self.base_dir
  }

  pub fn matches(&self, path: impl AsRef<Path>) -> bool {
    matches!(
      self.matches_detail(path),
      GlobMatchesDetail::Matched | GlobMatchesDetail::MatchedOptedOutExclude
    )
  }

  pub fn matches_detail(&self, path: impl AsRef<Path>) -> GlobMatchesDetail {
    let path = path.as_ref();
    let path = if path.is_absolute() {
      Cow::Borrowed(path)
    } else {
      Cow::Owned(self.base_dir.join(path))
    };
    let path = if let Ok(prefix) = path.strip_prefix(&self.base_dir) {
      Cow::Borrowed(prefix)
    } else {
      path
    };

    let matched_result = match self.check_exclude(&path, false) {
      ExcludeMatchDetail::Excluded => return GlobMatchesDetail::Excluded,
      ExcludeMatchDetail::OptedOutExclude => GlobMatchesDetail::MatchedOptedOutExclude,
      ExcludeMatchDetail::NotExcluded => GlobMatchesDetail::Matched,
    };

    if self.arg_include_matcher.as_ref().map(|m| m.include.is_match(&path)).unwrap_or(true)
      && self.config_include_matcher.as_ref().map(|m| m.is_match(&path)).unwrap_or(true)
    {
      matched_result
    } else {
      GlobMatchesDetail::NotMatched
    }
  }

  pub fn check_exclude(&self, path: &Path, is_dir: bool) -> ExcludeMatchDetail {
    let mut result = self.config_exclude_matcher.matched(path, is_dir);
    if let Some(matcher) = &self.arg_exclude_matcher {
      // arg excludes take precedence over config excludes when they match
      match matcher.matched(path, is_dir) {
        ExcludeMatchDetail::NotExcluded => {}
        arg_match => result = arg_match,
      }
    }
    result
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> ExcludeMatchDetail {
    let path = path.as_ref();
    if path.starts_with(&self.base_dir) {
      if let Some(include) = &self.arg_include_matcher {
        let has_any_dir = include.includes.iter().any(|base_pattern| base_pattern.matches_dir_for_traversal(path));
        if !has_any_dir {
          return ExcludeMatchDetail::Excluded;
        }
      }

      let path = path.strip_prefix(&self.base_dir).unwrap();
      self.check_exclude(path, true)
    } else {
      ExcludeMatchDetail::Excluded
    }
  }
}

/// The arg include matcher additionally keeps the include patterns around so
/// that directory traversal can be pruned (see `is_dir_ignored`).
#[derive(Debug)]
struct ArgIncludeMatcher {
  include: IncludeMatcher,
  includes: Vec<GlobPattern>,
}

/// Matches include patterns, with literal (non-glob) paths pulled into a hash
/// set fast path instead of being compiled into `matcher`.
///
/// This keeps the compiled glob regex small when a large number of literal file
/// paths are provided (e.g. when a shell expands a glob into thousands of
/// paths).
#[derive(Debug)]
struct IncludeMatcher {
  literal_paths: HashSet<PathBuf>,
  matcher: Override,
}

impl IncludeMatcher {
  fn is_match(&self, path: &Path) -> bool {
    self.literal_paths.contains(path) || matches!(self.matcher.matched(path, false), Match::Whitelist(_))
  }
}

/// Matches exclude patterns, with literal (non-glob) paths pulled into a hash
/// set fast path instead of being compiled into `matcher`. See `IncludeMatcher`.
struct ExcludeMatcher {
  literal_paths: HashSet<PathBuf>,
  matcher: Gitignore,
}

impl ExcludeMatcher {
  fn matched(&self, path: &Path, is_dir: bool) -> ExcludeMatchDetail {
    if self.literal_paths.contains(path) {
      return ExcludeMatchDetail::Excluded;
    }
    match self.matcher.matched(path, is_dir) {
      Match::None => ExcludeMatchDetail::NotExcluded,
      Match::Ignore(_) => ExcludeMatchDetail::Excluded,
      Match::Whitelist(_) => ExcludeMatchDetail::OptedOutExclude,
    }
  }
}

fn build_include_matcher(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<IncludeMatcher> {
  let use_fast_path = can_use_literal_fast_path(patterns, opts);
  let mut literal_paths = HashSet::new();
  let mut builder = OverrideBuilder::new(base_dir);
  builder.case_insensitive(!opts.case_sensitive)?;

  for pattern in patterns {
    if (pattern.is_literal() || use_fast_path)
      && let Some(path) = literal_relative_path(pattern)
    {
      literal_paths.insert(path);
    } else {
      add_override_pattern(&mut builder, pattern, base_dir)?;
    }
  }

  Ok(IncludeMatcher {
    literal_paths,
    matcher: builder.build().with_context(too_many_patterns_message)?,
  })
}

fn build_exclude_matcher(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<ExcludeMatcher> {
  let use_fast_path = can_use_literal_fast_path(patterns, opts);
  let mut literal_paths = HashSet::new();
  let mut builder = GitignoreBuilder::new(base_dir);
  builder.case_insensitive(!opts.case_sensitive)?;

  for pattern in patterns {
    if use_fast_path && let Some(path) = literal_relative_path(pattern) {
      literal_paths.insert(path);
    } else {
      builder.add_line(None, &normalize_pattern(pattern))?;
    }
  }

  Ok(ExcludeMatcher {
    literal_paths,
    matcher: builder.build().with_context(too_many_patterns_message)?,
  })
}

/// Gets whether literal patterns can be pulled into the hash set fast path.
///
/// This is only safe when matching case sensitively (the hash set lookup is case
/// sensitive) and when nothing is negated (a later negation can opt a literal
/// path back out, which only the order-aware compiled matcher handles correctly).
fn can_use_literal_fast_path(patterns: &[GlobPattern], opts: &GlobMatcherOptions) -> bool {
  opts.case_sensitive && !patterns.iter().any(|p| p.is_negated())
}

/// Gets the base-relative path for a literal pattern that can be matched by exact
/// path equality, or `None` when the pattern must go through the compiled matcher.
///
/// Only anchored patterns qualify: a bare basename (e.g. `foo.ts`) matches at any
/// depth and a directory-only pattern (trailing slash) depends on `is_dir`, so
/// neither is equivalent to an exact path lookup.
fn literal_relative_path(pattern: &GlobPattern) -> Option<PathBuf> {
  let is_literal = pattern.is_literal();
  let pattern = &pattern.relative_pattern;
  if !is_literal && is_pattern(pattern) {
    return None; // glob (also excludes negated patterns, which start with `!`)
  }
  let (anchored, relative) = if let Some(rest) = pattern.strip_prefix("./") {
    (true, rest)
  } else if let Some(rest) = pattern.strip_prefix('/') {
    (true, rest)
  } else {
    // anchored if it has an internal slash (a bare basename matches at any depth)
    (pattern.trim_end_matches('/').contains('/'), pattern.as_str())
  };
  if !anchored || relative.is_empty() || relative.ends_with('/') {
    return None;
  }
  Some(PathBuf::from(relative))
}

fn add_override_pattern(builder: &mut OverrideBuilder, pattern: &GlobPattern, base_dir: &CanonicalizedPathBuf) -> Result<()> {
  if pattern.base_dir != *base_dir {
    match pattern.clone().into_new_base(base_dir.clone()) {
      Some(pattern) => {
        builder.add(&normalize_pattern(&pattern))?;
      }
      None => {
        builder.add(&pattern.as_absolute_pattern_text())?;
      }
    }
  } else {
    builder.add(&normalize_pattern(pattern))?;
  }
  Ok(())
}

/// Surfaces a helpful message when the compiled glob exceeds the regex size
/// limit, which generally happens when a shell expands a glob into a huge
/// number of paths.
fn too_many_patterns_message() -> String {
  concat!(
    "Failed building glob matcher because the provided file patterns were too large to compile. ",
    "This usually happens when your shell expands a glob into many file paths. ",
    "Try quoting the glob so dprint expands it instead (ex. dprint fmt \"./**/*.ts\")."
  )
  .to_string()
}

fn normalize_pattern(pattern: &GlobPattern) -> Cow<'_, str> {
  // change patterns that start with ./ to be at the "root" of the globbing
  if pattern.relative_pattern.starts_with("!./") {
    Cow::Owned(format!("!/{}", &pattern.relative_pattern[3..]))
  } else if pattern.relative_pattern.starts_with("./") {
    Cow::Owned(format!("/{}", &pattern.relative_pattern[2..]))
  } else {
    Cow::Borrowed(&pattern.relative_pattern)
  }
}

fn get_base_dir<'a>(dirs: impl Iterator<Item = &'a CanonicalizedPathBuf>) -> Option<CanonicalizedPathBuf> {
  let mut base_dir: Option<&'a CanonicalizedPathBuf> = None;
  for dir in dirs {
    if let Some(base_dir) = base_dir.as_mut() {
      if base_dir.starts_with(dir) {
        *base_dir = dir;
      }
    } else {
      base_dir = Some(dir);
    }
  }
  base_dir.map(ToOwned::to_owned)
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn works() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/match.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/other/match.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/no-match.ts"), GlobMatchesDetail::Excluded);
  }

  #[test]
  fn cli_args_intersection_excludes_union() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(vec![GlobPattern::new("src/*.ts".to_string(), cwd.clone())]),
        config_includes: Some(vec![GlobPattern::new("*.ts".to_string(), cwd.clone())]),
        arg_excludes: Some(vec![GlobPattern::new("no-match2.ts".to_string(), cwd.clone())]),
        config_excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/match.ts"), GlobMatchesDetail::NotMatched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/src/match.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/other/match.ts"), GlobMatchesDetail::NotMatched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/no-match.ts"), GlobMatchesDetail::Excluded);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/src/no-match.ts"), GlobMatchesDetail::Excluded);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/src/no-match2.ts"), GlobMatchesDetail::Excluded);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/src/no-match3.ts"), GlobMatchesDetail::Matched);
  }

  #[test]
  fn cli_args_literal_paths_match() {
    // literal paths passed on the command line take the hash set fast path
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(vec![
          GlobPattern::new("./a.ts".to_string(), cwd.clone()),
          GlobPattern::new("./sub/b.ts".to_string(), cwd.clone()),
          GlobPattern::new("./glob/*.ts".to_string(), cwd.clone()),
        ]),
        config_includes: None,
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/b.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/glob/c.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/not-listed.ts"), GlobMatchesDetail::NotMatched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/not-listed.ts"), GlobMatchesDetail::NotMatched);
  }

  #[test]
  fn cli_args_many_literal_paths_do_not_exceed_regex_limit() {
    // a shell expanding a glob into a huge number of literal paths used to
    // overflow the compiled glob regex

    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let arg_includes = (0..50_000)
      .map(|i| GlobPattern::new(format!("./some/nested/directory/path/file_{i}.ts"), cwd.clone()))
      .collect();
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(arg_includes),
        config_includes: None,
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(
      glob_matcher.matches_detail("/testing/dir/some/nested/directory/path/file_42.ts"),
      GlobMatchesDetail::Matched
    );
    assert_eq!(
      glob_matcher.matches_detail("/testing/dir/some/nested/directory/path/missing.ts"),
      GlobMatchesDetail::NotMatched
    );
  }

  #[test]
  fn config_literal_includes_and_excludes_match() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![
          GlobPattern::new("./a.ts".to_string(), cwd.clone()),
          GlobPattern::new("sub/b.ts".to_string(), cwd.clone()),
        ]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("sub/c.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/b.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/c.ts"), GlobMatchesDetail::Excluded);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/not-listed.ts"), GlobMatchesDetail::NotMatched);
    // an anchored literal must not match the same name at a different depth
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/a.ts"), GlobMatchesDetail::NotMatched);
  }

  #[test]
  fn bare_basename_pattern_matches_at_any_depth() {
    // a non-anchored literal (no internal slash) keeps gitignore semantics of
    // matching the basename at any depth, so it must stay in the compiled matcher
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("a.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("excluded.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/nested/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/sub/excluded.ts"), GlobMatchesDetail::Excluded);
  }

  #[test]
  fn literal_dir_exclude_prunes_traversal() {
    // an anchored literal directory exclude should prune the directory during
    // traversal (its children are matched via pruning, not per-file matching)
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())]),
        arg_excludes: Some(vec![GlobPattern::new("./dist".to_string(), cwd.clone())]),
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.is_dir_ignored("/testing/dir/dist"), ExcludeMatchDetail::Excluded);
    assert_eq!(glob_matcher.is_dir_ignored("/testing/dir/src"), ExcludeMatchDetail::NotExcluded);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/src/a.ts"), GlobMatchesDetail::Matched);
  }

  #[test]
  fn negated_include_falls_back_to_compiled_matcher() {
    // a negated pattern in the group disables the literal fast path so the
    // order-dependent opt-out semantics are preserved
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![
          GlobPattern::new("./a.ts".to_string(), cwd.clone()),
          GlobPattern::new("./b.ts".to_string(), cwd.clone()),
          GlobPattern::new("!./a.ts".to_string(), cwd.clone()),
        ]),
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    // a.ts was opted back out by the later negation
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::NotMatched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/b.ts"), GlobMatchesDetail::Matched);
  }

  #[test]
  fn negated_exclude_opt_out_falls_back_to_compiled_matcher() {
    // a negation in the excludes opts a file back in; this must keep working
    // through the compiled gitignore matcher
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![
          GlobPattern::new("**/*.ts".to_string(), cwd.clone()),
          GlobPattern::new("!./keep.ts".to_string(), cwd.clone()),
        ],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/keep.ts"), GlobMatchesDetail::MatchedOptedOutExclude);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/other.ts"), GlobMatchesDetail::Excluded);
  }

  #[test]
  fn arg_exclude_takes_precedence_over_config_exclude() {
    // a literal arg exclude opting back in should override a config exclude
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())]),
        arg_excludes: Some(vec![GlobPattern::new("./keep.ts".to_string(), cwd.clone()).invert()]),
        config_excludes: vec![GlobPattern::new("**/*.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    // config excludes everything; the arg exclude opts keep.ts back in
    assert_eq!(glob_matcher.matches_detail("/testing/dir/keep.ts"), GlobMatchesDetail::MatchedOptedOutExclude);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/other.ts"), GlobMatchesDetail::Excluded);
  }

  #[test]
  fn case_insensitive_does_not_use_literal_fast_path() {
    // the literal fast path is case sensitive, so it must not be used when
    // matching case insensitively—matching still goes through the compiled matcher
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("./a.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: false,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/A.TS"), GlobMatchesDetail::Matched);
  }

  #[test]
  fn literal_pattern_with_glob_char_stays_in_matcher() {
    // patterns containing glob metacharacters must not take the literal fast path
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("./a?.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("/testing/dir/ab.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("/testing/dir/a.ts"), GlobMatchesDetail::NotMatched);
  }

  #[test]
  fn literal_match_with_relative_path_input() {
    // matches_detail should resolve relative input against the base dir before
    // the literal lookup
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(vec![GlobPattern::new("./sub/a.ts".to_string(), cwd.clone())]),
        config_includes: None,
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("sub/a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("sub/b.ts"), GlobMatchesDetail::NotMatched);
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn literal_match_with_backslash_separators() {
    // the literal lookup compares paths component-wise, so a path with backslash
    // separators must match a literal stored with forward slashes
    let cwd = CanonicalizedPathBuf::new_for_testing("C:\\testing\\dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(vec![GlobPattern::new("./sub/a.ts".to_string(), cwd.clone())]),
        config_includes: None,
        arg_excludes: None,
        config_excludes: vec![],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.matches_detail("C:\\testing\\dir\\sub\\a.ts"), GlobMatchesDetail::Matched);
    assert_eq!(glob_matcher.matches_detail("C:\\testing\\dir\\sub\\b.ts"), GlobMatchesDetail::NotMatched);
  }

  #[test]
  fn cli_args_is_dir_excluded() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: Some(vec![GlobPattern::new("src/*.ts".to_string(), cwd.clone())]),
        config_includes: Some(vec![GlobPattern::new("*.ts".to_string(), cwd.clone())]),
        arg_excludes: Some(vec![GlobPattern::new("no-match2.ts".to_string(), cwd.clone())]),
        config_excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert_eq!(glob_matcher.is_dir_ignored("/testing/dir/src/"), ExcludeMatchDetail::NotExcluded);
    assert_eq!(glob_matcher.is_dir_ignored("/testing/dir/other/"), ExcludeMatchDetail::Excluded);
    assert_eq!(glob_matcher.is_dir_ignored("/testing/dir/src/sub"), ExcludeMatchDetail::Excluded);
  }

  #[test]
  fn handles_external_files() {
    let cwd = CanonicalizedPathBuf::new_for_testing("/testing/dir");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("/testing/dir/*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("/testing/dir/no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert!(!glob_matcher.matches("/some/other/dir/file.ts"));
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn works_unc_paths() {
    let cwd = CanonicalizedPathBuf::new_for_testing("\\?\\UNC\\wsl$\\Ubuntu\\home\\david");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        arg_includes: None,
        config_includes: Some(vec![GlobPattern::new("*.ts".to_string(), cwd.clone())]),
        arg_excludes: None,
        config_excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions {
        case_sensitive: true,
        base_dir: cwd,
      },
    )
    .unwrap();
    assert!(glob_matcher.matches("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\match.ts"));
    assert!(glob_matcher.matches("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\dir\\other.ts"));
    assert!(!glob_matcher.matches("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\no-match.ts"));
  }
}
