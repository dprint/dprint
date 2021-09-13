use std::path::Path;

use dprint_cli_core::types::ErrBox;
use globset::GlobBuilder;
use globset::GlobSet;
use globset::GlobSetBuilder;

use super::is_negated_glob;

pub struct GlobMatcherOptions {
  pub case_insensitive: bool,
}

pub struct GlobMatcher {
  include_globset: GlobSet,
  exclude_globset: GlobSet,
}

impl GlobMatcher {
  pub fn new(patterns: &[String], opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let mut match_patterns = Vec::new();
    let mut ignore_patterns = Vec::new();
    for pattern in patterns {
      if is_negated_glob(pattern) {
        ignore_patterns.push(pattern[1..].to_string());
      } else {
        match_patterns.push(pattern.to_string());
      }
    }
    Ok(GlobMatcher {
      include_globset: build_glob_set(&match_patterns, opts)?,
      exclude_globset: build_glob_set(&ignore_patterns, opts)?,
    })
  }

  pub fn is_match(&self, pattern: impl AsRef<Path>) -> bool {
    self.include_globset.is_match(&pattern) && !self.exclude_globset.is_match(&pattern)
  }

  pub fn is_ignored(&self, pattern: impl AsRef<Path>) -> bool {
    self.exclude_globset.is_match(&pattern)
  }
}

fn build_glob_set(file_patterns: &[String], opts: &GlobMatcherOptions) -> Result<GlobSet, ErrBox> {
  let mut builder = GlobSetBuilder::new();
  for pattern in file_patterns {
    builder.add(GlobBuilder::new(&pattern).case_insensitive(opts.case_insensitive).build()?);
  }
  return Ok(builder.build().unwrap());
}
