use std::borrow::Cow;
use std::path::{Path, PathBuf};

use dprint_cli_core::types::ErrBox;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::environment::{DirEntryKind, Environment};

pub fn glob(environment: &impl Environment, base: impl AsRef<Path>, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
  if file_patterns.iter().all(|p| is_negated_glob(p)) {
    // performance improvement (see issue #379)
    log_verbose!(environment, "Skipping negated globs: {:?}", file_patterns);
    return Ok(Vec::with_capacity(0));
  }

  let start_instant = std::time::Instant::now();
  log_verbose!(environment, "Globbing: {:?}", file_patterns);

  let glob_matcher = GlobMatcher::new(
    file_patterns,
    &GlobMatcherOptions {
      case_insensitive: cfg!(windows),
    },
  )?;
  let mut results = Vec::new();

  let mut pending_dirs = vec![base.as_ref().to_path_buf()];

  while !pending_dirs.is_empty() {
    let entries = environment.dir_info(pending_dirs.pop().unwrap())?;
    for entry in entries.into_iter() {
      match entry.kind {
        DirEntryKind::Directory => {
          if !glob_matcher.is_ignored(&entry.path) {
            pending_dirs.push(entry.path);
          }
        }
        DirEntryKind::File => {
          if glob_matcher.is_match(&entry.path) {
            results.push(entry.path);
          }
        }
      }
    }
  }

  log_verbose!(environment, "File(s) matched: {:?}", results);
  log_verbose!(environment, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

  Ok(results)
}

pub fn to_absolute_globs(file_patterns: Vec<String>, base_dir: &str) -> Vec<String> {
  file_patterns.into_iter().map(|p| to_absolute_glob(&p, base_dir)).collect()
}

pub fn to_absolute_glob(pattern: &str, dir: &str) -> String {
  // Adapted from https://github.com/dsherret/ts-morph/blob/0f8a77a9fa9d74e32f88f36992d527a2f059c6ac/packages/common/src/fileSystem/FileUtils.ts#L272

  // convert backslashes to forward slashes (don't worry about matching file names with back slashes)
  let mut pattern = pattern.replace("\\", "/");
  let dir = dir.replace("\\", "/");

  // check to see if glob is negated
  let is_negated = is_negated_glob(&pattern);
  if is_negated {
    pattern.drain(..1); // remove the leading "!"
  }

  // .gitignore: "If there is a separator at the beginning or middle (or both) of
  // the pattern, then the pattern is relative to the directory level of the particular
  // .gitignore file itself. Otherwise the pattern may also match at any level below the
  // .gitignore level."
  let is_relative = match pattern.find("/") {
    Some(index) => index != pattern.len() - 1, // not the end of the pattern
    None => false,
  };

  // trim starting ./ from glob patterns
  if pattern.starts_with("./") {
    pattern.drain(..2);
  }

  // when the glob pattern is only a . use an empty string
  if pattern == "." {
    pattern = String::new();
  }

  // store last character before glob is modified
  let suffix = pattern.chars().last();

  // make glob absolute
  if !is_absolute_pattern(&pattern) {
    if is_relative || pattern.starts_with("**/") || pattern.trim().is_empty() {
      pattern = glob_join(dir, pattern);
    } else {
      pattern = glob_join(dir, format!("**/{}", pattern));
    }
  }

  // if glob had a trailing `/`, re-add it back
  if suffix == Some('/') && !pattern.ends_with('/') {
    pattern.push('/');
  }

  if is_negated {
    format!("!{}", pattern)
  } else {
    pattern
  }
}

pub fn is_negated_glob(pattern: &str) -> bool {
  let mut chars = pattern.chars();
  let first_char = chars.next();
  let second_char = chars.next();

  return first_char == Some('!') && second_char != Some('(');
}

fn glob_join(dir: String, pattern: String) -> String {
  // strip trailing slash
  let dir = if dir.ends_with('/') {
    Cow::Borrowed(&dir[..dir.len() - 1])
  } else {
    Cow::Owned(dir)
  };
  // strip leading slash
  let pattern = if pattern.starts_with('/') {
    Cow::Borrowed(&pattern[1..])
  } else {
    Cow::Owned(pattern)
  };

  if pattern.len() == 0 {
    dir.into_owned()
  } else {
    format!("{}/{}", dir, pattern)
  }
}

pub fn is_absolute_pattern(pattern: &str) -> bool {
  let pattern = if is_negated_glob(pattern) { &pattern[1..] } else { &pattern };
  pattern.starts_with("/") || is_windows_absolute_pattern(pattern)
}

fn is_windows_absolute_pattern(pattern: &str) -> bool {
  // ex. D:/
  let mut chars = pattern.chars();

  // ensure the first character is alphabetic
  let next_char = chars.next();
  if next_char.is_none() || !next_char.unwrap().is_ascii_alphabetic() {
    return false;
  }

  // skip over the remaining alphabetic characters
  let mut next_char = chars.next();
  while next_char.is_some() && next_char.unwrap().is_ascii_alphabetic() {
    next_char = chars.next();
  }

  // ensure colon
  if next_char != Some(':') {
    return false;
  }

  // now check for the last slash
  let next_char = chars.next();
  matches!(next_char, Some('/'))
}

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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn it_should_get_if_absolute_pattern() {
    assert_eq!(is_absolute_pattern("test.ts"), false);
    assert_eq!(is_absolute_pattern("!test.ts"), false);
    assert_eq!(is_absolute_pattern("/test.ts"), true);
    assert_eq!(is_absolute_pattern("!/test.ts"), true);
    assert_eq!(is_absolute_pattern("D:/test.ts"), true);
    assert_eq!(is_absolute_pattern("!D:/test.ts"), true);
  }

  #[test]
  fn it_should_get_absolute_globs() {
    assert_eq!(to_absolute_glob("**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test/"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("D:/**/*.ts", "/test/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "D:/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob(".", "D:\\test"), "D:/test");
    assert_eq!(to_absolute_glob("\\test\\asdf.ts", "D:\\test"), "/test/asdf.ts");
    assert_eq!(to_absolute_glob("!**/*.ts", "D:\\test"), "!D:/test/**/*.ts");
    assert_eq!(to_absolute_glob("///test/**/*.ts", "D:\\test"), "///test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "CD:\\"), "CD:/**/*.ts");

    assert_eq!(to_absolute_glob("./test.ts", "/test/"), "/test/test.ts");
    assert_eq!(to_absolute_glob("test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("*/test.ts", "/test/"), "/test/*/test.ts");
    assert_eq!(to_absolute_glob("*test.ts", "/test/"), "/test/**/*test.ts");
    assert_eq!(to_absolute_glob("**/test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("/test.ts", "/test/"), "/test.ts");
    assert_eq!(to_absolute_glob("test/", "/test/"), "/test/**/test/");

    assert_eq!(to_absolute_glob("!./test.ts", "/test/"), "!/test/test.ts");
    assert_eq!(to_absolute_glob("!test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!*/test.ts", "/test/"), "!/test/*/test.ts");
    assert_eq!(to_absolute_glob("!*test.ts", "/test/"), "!/test/**/*test.ts");
    assert_eq!(to_absolute_glob("!**/test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!/test.ts", "/test/"), "!/test.ts");
    assert_eq!(to_absolute_glob("!test/", "/test/"), "!/test/**/test/");
    // has a slash in the middle, so it's relative
    assert_eq!(to_absolute_glob("test/test.ts", "/test/"), "/test/test/test.ts");
  }
}
