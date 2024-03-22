use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
use ignore::gitignore::Gitignore;
use ignore::gitignore::GitignoreBuilder;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;
use ignore::Match;

use crate::environment::CanonicalizedPathBuf;

use super::GlobPattern;
use super::GlobPatterns;

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
  config_include_matcher: Option<Override>,
  arg_include_matcher: Option<Override>,
  config_exclude_matcher: Gitignore,
  arg_exclude_matcher: Option<Gitignore>,
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
        Some(includes) => Some(build_override(&includes, opts, &base_dir)?),
        None => None,
      },
      arg_include_matcher: match arg_includes {
        Some(includes) => Some(build_override(&includes, opts, &base_dir)?),
        None => None,
      },
      config_exclude_matcher: build_gitignore(&config_excludes, opts, &base_dir)?,
      arg_exclude_matcher: match arg_excludes {
        Some(excludes) => Some(build_gitignore(&excludes, opts, &base_dir)?),
        None => None,
      },
      base_dir,
    })
  }

  pub fn base_dir(&self) -> &CanonicalizedPathBuf {
    &self.base_dir
  }

  /// Gets if the matcher only has excludes patterns.
  pub fn has_only_excludes(&self) -> bool {
    (self.config_include_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true) && self.arg_include_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true))
      && (!self.config_exclude_matcher.is_empty() || !self.arg_exclude_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true))
  }

  pub fn matches(&self, path: impl AsRef<Path>) -> bool {
    matches!(
      self.matches_detail(path),
      GlobMatchesDetail::Matched | GlobMatchesDetail::MatchedOptedOutExclude
    )
  }

  pub fn matches_detail(&self, path: impl AsRef<Path>) -> GlobMatchesDetail {
    let path = path.as_ref();
    let path = if path.is_absolute() && path.starts_with(&self.base_dir) {
      if let Ok(prefix) = path.strip_prefix(&self.base_dir) {
        Cow::Borrowed(prefix)
      } else {
        // this is a very strange state that we want to know more about,
        // so just always log directly to stderr in this scenario and maybe
        // eventually remove this code.
        eprintln!(
          "WARNING: Path prefix error for {} and {}. Please report this error in issue #540.",
          &self.base_dir.display(),
          path.display()
        );
        return GlobMatchesDetail::NotMatched;
      }
    } else if !path.is_absolute() {
      Cow::Owned(self.base_dir.join(path))
    } else {
      Cow::Borrowed(path)
    };

    let matched_result = match self.check_exclude(&path, false) {
      ExcludeMatchDetail::Excluded => return GlobMatchesDetail::Excluded,
      ExcludeMatchDetail::OptedOutExclude => GlobMatchesDetail::MatchedOptedOutExclude,
      ExcludeMatchDetail::NotExcluded => GlobMatchesDetail::Matched,
    };

    if self
      .arg_include_matcher
      .as_ref()
      .map(|m| matches!(m.matched(&path, false), Match::Whitelist(_)))
      .unwrap_or(true)
      && self
        .config_include_matcher
        .as_ref()
        .map(|m| matches!(m.matched(&path, false), Match::Whitelist(_)))
        .unwrap_or(true)
    {
      matched_result
    } else {
      GlobMatchesDetail::NotMatched
    }
  }

  pub fn check_exclude(&self, path: &Path, is_dir: bool) -> ExcludeMatchDetail {
    let config_match = self.config_exclude_matcher.matched(&path, is_dir);
    let mut result = match config_match {
      Match::None => ExcludeMatchDetail::NotExcluded,
      Match::Ignore(_) => ExcludeMatchDetail::Excluded,
      Match::Whitelist(_) => ExcludeMatchDetail::OptedOutExclude,
    };
    if let Some(matcher) = &self.arg_exclude_matcher {
      let arg_match = matcher.matched(path, is_dir);
      match arg_match {
        Match::None => {}
        Match::Ignore(_) => {
          result = ExcludeMatchDetail::Excluded;
        }
        Match::Whitelist(_) => {
          result = ExcludeMatchDetail::OptedOutExclude;
        }
      }
    }
    result
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> ExcludeMatchDetail {
    if path.as_ref().starts_with(&self.base_dir) {
      let path = path.as_ref().strip_prefix(&self.base_dir).unwrap();
      self.check_exclude(path, true)
    } else {
      ExcludeMatchDetail::Excluded
    }
  }
}

fn build_override(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<Override> {
  let mut builder = OverrideBuilder::new(base_dir);
  let builder = builder.case_insensitive(!opts.case_sensitive)?;

  for pattern in patterns {
    let pattern = normalize_pattern(pattern);
    builder.add(&pattern)?;
  }

  Ok(builder.build()?)
}

fn build_gitignore(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<Gitignore> {
  let mut builder = GitignoreBuilder::new(base_dir);
  let builder = builder.case_insensitive(!opts.case_sensitive)?;

  for pattern in patterns {
    let pattern = normalize_pattern(pattern);
    builder.add_line(None, &pattern)?;
  }

  Ok(builder.build()?)
}

fn normalize_pattern(pattern: &GlobPattern) -> Cow<str> {
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
    assert!(!glob_matcher.has_only_excludes());
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
    assert!(!glob_matcher.has_only_excludes());
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
