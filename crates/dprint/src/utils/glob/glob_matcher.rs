use std::path::Path;
use std::path::PathBuf;

use dprint_cli_core::types::ErrBox;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;
use ignore::Match;

use crate::utils::is_negated_glob;

use super::strip_slash_start_pattern;
use super::GlobPattern;
use super::GlobPatterns;

pub struct GlobMatcherOptions {
  pub case_insensitive: bool,
}

pub struct GlobMatcher {
  base_dir: PathBuf,
  include_matcher: Override,
  exclude_matcher: Override,
}

impl GlobMatcher {
  pub fn new(patterns: GlobPatterns, opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let base_dir = get_base_dir(
      patterns
        .includes
        .iter()
        .map(|p| &p.base_dir)
        .chain(patterns.excludes.iter().map(|p| &p.base_dir)),
    )
    .unwrap_or_else(|| {
      // just use a dummy path, no directories means this won't ever be matched against
      PathBuf::from("./")
    });

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
      include_matcher: build_override(&includes, opts, &base_dir)?,
      exclude_matcher: build_override(&excludes, opts, &base_dir)?,
      base_dir,
    })
  }

  pub fn is_match(&self, path: impl AsRef<Path>) -> bool {
    let path = path.as_ref().strip_prefix(&self.base_dir).unwrap();
    matches!(self.include_matcher.matched(&path, false), Match::Whitelist(_)) && !matches!(self.exclude_matcher.matched(&path, false), Match::Whitelist(_))
  }

  pub fn is_dir_ignored(&self, path: impl AsRef<Path>) -> bool {
    let path = path.as_ref().strip_prefix(&self.base_dir).unwrap();
    matches!(self.exclude_matcher.matched(&path, true), Match::Whitelist(_))
  }
}

fn build_override(patterns: &[GlobPattern], opts: &GlobMatcherOptions, base_dir: &Path) -> Result<Override, ErrBox> {
  let mut builder = OverrideBuilder::new(&base_dir);
  let builder = builder.case_insensitive(opts.case_insensitive)?;

  for pattern in patterns {
    // todo: bug here
    // println!("-------");
    // println!("{}", pattern.relative_pattern);

    let pattern = if pattern.relative_pattern.contains("/") {
      strip_slash_start_pattern(&pattern.relative_pattern)
    } else {
      if is_negated_glob(&pattern.relative_pattern) {
        format!("!**/{}", &pattern.relative_pattern[1..])
      } else {
        format!("**/{}", pattern.relative_pattern)
      }
    };
    builder.add(&pattern)?;
  }

  Ok(builder.build()?)
}

fn get_base_dir<'a>(dirs: impl Iterator<Item = &'a PathBuf>) -> Option<PathBuf> {
  let mut base_dir: Option<&'a PathBuf> = None;
  for dir in dirs {
    if let Some(base_dir) = base_dir.as_mut() {
      if base_dir.starts_with(dir) {
        *base_dir = dir;
      }
    } else {
      base_dir = Some(dir);
    }
  }
  base_dir.map(|d| d.to_owned())
}
