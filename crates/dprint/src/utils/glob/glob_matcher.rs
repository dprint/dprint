use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
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
  /// Matched an excludes pattern.
  Excluded,
  /// Matched neither an includes or excludes pattern.
  NotMatched,
}

pub struct GlobMatcher {
  base_dir: CanonicalizedPathBuf,
  config_include_matcher: Option<Override>,
  arg_include_matcher: Option<Override>,
  config_exclude_matcher: Override,
  arg_exclude_matcher: Option<Override>,
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
      .filter_map(|pattern| pattern.into_non_negated().into_new_base(base_dir.clone()))
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
        .filter_map(|pattern| pattern.into_non_negated().into_new_base(base_dir.clone()))
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
      config_exclude_matcher: build_override(&config_excludes, opts, &base_dir)?,
      arg_exclude_matcher: match arg_excludes {
        Some(excludes) => Some(build_override(&excludes, opts, &base_dir)?),
        None => None,
      },
      base_dir,
    })
  }

  /// Gets if the matcher only has excludes patterns.
  pub fn has_only_excludes(&self) -> bool {
    (self.config_include_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true) && self.arg_include_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true))
      && (!self.config_exclude_matcher.is_empty() || !self.arg_exclude_matcher.as_ref().map(|m| m.is_empty()).unwrap_or(true))
  }

  pub fn matches(&self, path: impl AsRef<Path>) -> bool {
    self.matches_detail(path) == GlobMatchesDetail::Matched
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

    if matches!(self.config_exclude_matcher.matched(&path, false), Match::Whitelist(_))
      || self
        .arg_exclude_matcher
        .as_ref()
        .map(|m| matches!(m.matched(&path, false), Match::Whitelist(_)))
        .unwrap_or(false)
    {
      GlobMatchesDetail::Excluded
    } else if self
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
      GlobMatchesDetail::Matched
    } else {
      GlobMatchesDetail::NotMatched
    }
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> bool {
    if path.as_ref().starts_with(&self.base_dir) {
      let path = path.as_ref().strip_prefix(&self.base_dir).unwrap();
      matches!(self.config_exclude_matcher.matched(path, true), Match::Whitelist(_))
        || self
          .arg_exclude_matcher
          .as_ref()
          .map(|m| matches!(m.matched(path, true), Match::Whitelist(_)))
          .unwrap_or(false)
    } else {
      true
    }
  }

  /// More expensive check for if the directory is already ignored.
  /// Prefer using `matches` if you already know the parent directory
  /// isn't ignored as it's faster.
  pub fn matches_and_dir_not_ignored(&self, file_path: impl AsRef<Path>) -> bool {
    if !self.matches(&file_path) {
      return false;
    }

    if file_path.as_ref().starts_with(&self.base_dir) {
      for ancestor in file_path.as_ref().ancestors() {
        if let Ok(path) = ancestor.strip_prefix(&self.base_dir) {
          if matches!(self.config_exclude_matcher.matched(path, true), Match::Whitelist(_)) {
            return false;
          }
          if let Some(arg_exclude_matcher) = &self.arg_exclude_matcher {
            if matches!(arg_exclude_matcher.matched(path, true), Match::Whitelist(_)) {
              return false;
            }
          }
        } else {
          return true;
        }
      }
      true
    } else {
      false
    }
  }
}

fn build_override(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<Override> {
  let mut builder = OverrideBuilder::new(base_dir);
  let builder = builder.case_insensitive(!opts.case_sensitive)?;

  for pattern in patterns {
    // change patterns that start with ./ to be at the "root" of the globbing
    let pattern = if pattern.relative_pattern.starts_with("!./") {
      Cow::Owned(format!("!/{}", &pattern.relative_pattern[3..]))
    } else if pattern.relative_pattern.starts_with("./") {
      Cow::Owned(format!("/{}", &pattern.relative_pattern[2..]))
    } else {
      Cow::Borrowed(&pattern.relative_pattern)
    };
    builder.add(&pattern)?;
  }

  Ok(builder.build()?)
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

  #[test]
  fn handles_ignored_dir() {
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
    assert!(glob_matcher.matches_and_dir_not_ignored("/testing/dir/match.ts"));
    assert!(glob_matcher.matches_and_dir_not_ignored("/testing/dir/other/match.ts"));
    assert!(!glob_matcher.matches_and_dir_not_ignored("/testing/sub-dir/no-match.ts"));
    assert!(!glob_matcher.matches_and_dir_not_ignored("/testing/sub-dir/nested/no-match.ts"));
  }

  #[test]
  fn handles_ignored_dir_while_include_is_sub_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/");
    let cwd = CanonicalizedPathBuf::new_for_testing("/sub-dir");
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
    assert!(glob_matcher.matches_and_dir_not_ignored("/sub-dir/dir/match.ts"));
    assert!(glob_matcher.matches_and_dir_not_ignored("/sub-dir/dir/other/match.ts"));
    assert!(!glob_matcher.matches_and_dir_not_ignored("/sub-dir/dist/no-match.ts"));
  }
}
