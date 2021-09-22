use std::path::Path;

use dprint_cli_core::types::ErrBox;
use globset::GlobBuilder;
use globset::GlobSet;
use globset::GlobSetBuilder;

use super::is_negated_glob;

#[derive(Debug)]
pub struct GlobPatterns {
  pub includes: Vec<String>,
  pub excludes: Vec<String>,
}

pub struct GlobMatcherOptions {
  pub case_insensitive: bool,
}

pub struct GlobMatcher {
  include_globset: GlobSet,
  exclude_globset: GlobSet,
}

impl GlobMatcher {
  pub fn new(patterns: &GlobPatterns, opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let excludes = patterns
      .excludes
      .iter()
      .map(|pattern| if is_negated_glob(pattern) { &pattern[1..] } else { pattern })
      .collect::<Vec<_>>();
    Ok(GlobMatcher {
      include_globset: build_glob_set(&patterns.includes, opts)?,
      exclude_globset: build_glob_set(&excludes, opts)?,
    })
  }

  pub fn is_match(&self, pattern: impl AsRef<Path>) -> bool {
    self.include_globset.is_match(&pattern) && !self.exclude_globset.is_match(&pattern)
  }

  pub fn is_ignored(&self, pattern: impl AsRef<Path>) -> bool {
    self.exclude_globset.is_match(&pattern)
  }
}

fn build_glob_set(file_patterns: &[impl AsRef<str>], opts: &GlobMatcherOptions) -> Result<GlobSet, ErrBox> {
  let mut builder = GlobSetBuilder::new();
  for pattern in file_patterns {
    builder.add(GlobBuilder::new(pattern.as_ref()).case_insensitive(opts.case_insensitive).build()?);
  }
  return Ok(builder.build().unwrap());
}
