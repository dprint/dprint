use std::borrow::Cow;
use std::path::Path;

use dprint_cli_core::types::ErrBox;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;
use ignore::Match;

use crate::environment::CanonicalizedPathBuf;

use super::GlobPattern;
use super::GlobPatterns;

pub struct GlobMatcherOptions {
  pub case_sensitive: bool,
}

pub struct GlobMatcher {
  inner: GlobMatcherInner,
}

enum GlobMatcherInner {
  Empty,
  Matcher {
    base_dir: CanonicalizedPathBuf,
    include_matcher: Override,
    exclude_matcher: Override,
  },
}

impl GlobMatcher {
  pub fn new(patterns: GlobPatterns, opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let base_dir = get_base_dir(
      patterns
        .includes
        .iter()
        .map(|p| &p.base_dir)
        .chain(patterns.excludes.iter().map(|p| &p.base_dir)),
    );

    let base_dir = if let Some(base_dir) = base_dir {
      base_dir
    } else {
      return Ok(GlobMatcher {
        inner: GlobMatcherInner::Empty,
      });
    };

    // map the includes and excludes to have a new base
    let excludes = patterns
      .excludes
      .into_iter()
      .map(|pattern| pattern.into_non_negated().into_new_base(base_dir.clone()))
      .collect::<Vec<_>>();
    let includes = patterns
      .includes
      .into_iter()
      .map(|pattern| pattern.into_new_base(base_dir.clone()))
      .collect::<Vec<_>>();

    Ok(GlobMatcher {
      inner: GlobMatcherInner::Matcher {
        include_matcher: build_override(&includes, opts, &base_dir)?,
        exclude_matcher: build_override(&excludes, opts, &base_dir)?,
        base_dir,
      },
    })
  }

  pub fn is_match(&self, path: impl AsRef<Path>) -> bool {
    match &self.inner {
      GlobMatcherInner::Empty => false,
      GlobMatcherInner::Matcher {
        base_dir,
        include_matcher,
        exclude_matcher,
      } => {
        let path = path.as_ref();
        let path = if path.is_absolute() {
          Cow::Borrowed(path.strip_prefix(&base_dir).unwrap())
        } else {
          Cow::Owned(base_dir.join(path))
        };
        matches!(include_matcher.matched(&path, false), Match::Whitelist(_)) && !matches!(exclude_matcher.matched(&path, false), Match::Whitelist(_))
      }
    }
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> bool {
    match &self.inner {
      GlobMatcherInner::Empty => false,
      GlobMatcherInner::Matcher { base_dir, exclude_matcher, .. } => {
        let path = path.as_ref().strip_prefix(&base_dir).unwrap();
        matches!(exclude_matcher.matched(&path, true), Match::Whitelist(_))
      }
    }
  }
}

fn build_override(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &CanonicalizedPathBuf) -> Result<Override, ErrBox> {
  let mut builder = OverrideBuilder::new(&base_dir);
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
        includes: vec![GlobPattern::new("*.ts".to_string(), cwd.clone())],
        excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions { case_sensitive: true },
    )
    .unwrap();
    assert!(glob_matcher.is_match("/testing/dir/match.ts"));
    assert!(glob_matcher.is_match("/testing/dir/other/match.ts"));
    assert!(!glob_matcher.is_match("/testing/dir/no-match.ts"));
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn works_unc_paths() {
    let cwd = CanonicalizedPathBuf::new_for_testing("\\?\\UNC\\wsl$\\Ubuntu\\home\\david");
    let glob_matcher = GlobMatcher::new(
      GlobPatterns {
        includes: vec![GlobPattern::new("*.ts".to_string(), cwd.clone())],
        excludes: vec![GlobPattern::new("no-match.ts".to_string(), cwd.clone())],
      },
      &GlobMatcherOptions { case_sensitive: true },
    )
    .unwrap();
    assert!(glob_matcher.is_match("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\match.ts"));
    assert!(glob_matcher.is_match("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\dir\\other.ts"));
    assert!(!glob_matcher.is_match("\\?\\UNC\\wsl$\\Ubuntu\\home\\david\\no-match.ts"));
  }
}
