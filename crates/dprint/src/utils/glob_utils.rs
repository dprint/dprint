use std::borrow::Cow;
use std::path::Path;

use dprint_cli_core::types::ErrBox;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

// Adapted from https://github.com/dsherret/ts-morph/blob/0f8a77a9fa9d74e32f88f36992d527a2f059c6ac/packages/common/src/fileSystem/FileUtils.ts#L272

pub fn to_absolute_globs(file_patterns: &Vec<String>, base_dir: &str) -> Vec<String> {
  file_patterns.iter().map(|p| to_absolute_glob(p, base_dir)).collect()
}

pub fn to_absolute_glob(pattern: &str, dir: &str) -> String {
  // convert backslashes to forward slashes (don't worry about matching file names with back slashes)
  let mut pattern = pattern.replace("\\", "/");
  let dir = dir.replace("\\", "/");

  // trim starting ./ from glob patterns
  if pattern.starts_with("./") {
    pattern = pattern.chars().skip(2).collect();
  }

  // when the glob pattern is only a . use an empty string
  if pattern == "." {
    pattern = String::new();
  }

  // store last character before glob is modified
  let suffix = pattern.chars().last();

  // check to see if glob is negated (and not a leading negated-extglob)
  let is_negated = is_negated_glob(&pattern);
  if is_negated {
    pattern = pattern.chars().skip(1).collect(); // remove the leading "!"
  }

  // make glob absolute
  if !is_absolute_path(&pattern) {
    pattern = glob_join(dir, pattern);
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

fn is_absolute_path(file_path: &str) -> bool {
  file_path.starts_with("/") || is_windows_absolute_path(file_path)
}

fn is_windows_absolute_path(file_path: &str) -> bool {
  // ex. D:/
  let mut chars = file_path.chars();

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
  next_char == Some('/')
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
  fn it_should_get_absolute_globs() {
    assert_eq!("/**/*.ts", to_absolute_glob("**/*.ts", "/"));
    assert_eq!("/**/*.ts", to_absolute_glob("/**/*.ts", "/"));
    assert_eq!("/test/**/*.ts", to_absolute_glob("**/*.ts", "/test"));
    assert_eq!("/test/**/*.ts", to_absolute_glob("**/*.ts", "/test/"));
    assert_eq!("/**/*.ts", to_absolute_glob("/**/*.ts", "/test"));
    assert_eq!("/**/*.ts", to_absolute_glob("/**/*.ts", "/test/"));
    assert_eq!("D:/**/*.ts", to_absolute_glob("D:/**/*.ts", "/test/"));
    assert_eq!("D:/**/*.ts", to_absolute_glob("**/*.ts", "D:/"));
    assert_eq!("D:/test", to_absolute_glob(".", "D:\\test"));
    assert_eq!("/test/asdf.ts", to_absolute_glob("\\test\\asdf.ts", "D:\\test"));
    assert_eq!("!D:/test/**/*.ts", to_absolute_glob("!**/*.ts", "D:\\test"));
    assert_eq!("///test/**/*.ts", to_absolute_glob("///test/**/*.ts", "D:\\test"));
    assert_eq!("CD:/**/*.ts", to_absolute_glob("**/*.ts", "CD:\\"));
  }
}
