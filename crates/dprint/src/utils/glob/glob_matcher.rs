use std::path::Path;
use std::path::PathBuf;

use dprint_cli_core::types::ErrBox;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;
use ignore::Match;

use super::is_negated_glob;

#[derive(Debug)]
pub struct GlobPatterns {
  pub includes: Vec<String>,
  pub excludes: Vec<String>,
}

pub struct GlobMatcherOptions {
  pub base_dir: PathBuf,
  pub case_insensitive: bool,
}

pub struct GlobMatcher {
  include_matcher: Override,
  exclude_matcher: Override,
}

impl GlobMatcher {
  pub fn new(patterns: &GlobPatterns, opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let excludes = patterns
      .excludes
      .iter()
      .map(|pattern| if is_negated_glob(pattern) { &pattern[1..] } else { pattern })
      .collect::<Vec<_>>();
    Ok(GlobMatcher {
      include_matcher: build_override(&patterns.includes, opts)?,
      exclude_matcher: build_override(&excludes, opts)?,
    })
  }

  pub fn is_match(&self, path: impl AsRef<Path>) -> bool {
    matches!(self.include_matcher.matched(path.as_ref(), false), Match::Whitelist(_))
      && !matches!(self.exclude_matcher.matched(path.as_ref(), false), Match::Whitelist(_))
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> bool {
    matches!(self.exclude_matcher.matched(path.as_ref(), true), Match::Whitelist(_))
  }
}

fn build_override(patterns: &[impl AsRef<str>], opts: &GlobMatcherOptions) -> Result<Override, ErrBox> {
  let mut builder = OverrideBuilder::new(&opts.base_dir);
  let builder = builder.case_insensitive(opts.case_insensitive)?;

  for pattern in patterns {
    builder.add(pattern.as_ref())?;
  }

  return Ok(builder.build()?);
}
