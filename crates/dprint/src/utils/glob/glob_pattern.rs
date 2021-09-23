use std::path::PathBuf;

use super::is_negated_glob;
use super::to_absolute_glob;

#[derive(Debug, PartialEq)]
pub struct GlobPattern {
  pub relative_pattern: String,
  pub base_dir: PathBuf,
}

impl GlobPattern {
  pub fn new(relative_pattern: String, base_dir: PathBuf) -> Self {
    GlobPattern { relative_pattern, base_dir }
  }

  pub fn new_vec(relative_patterns: Vec<String>, base_dir: PathBuf) -> Vec<Self> {
    relative_patterns
      .into_iter()
      .map(|relative_pattern| GlobPattern::new(relative_pattern, base_dir.clone()))
      .collect()
  }

  pub fn absolute_pattern(&self) -> String {
    to_absolute_glob(&self.relative_pattern, &self.base_dir.to_string_lossy())
  }

  pub fn is_negated(&self) -> bool {
    is_negated_glob(&self.relative_pattern)
  }

  pub fn into_non_negated(self) -> GlobPattern {
    if self.is_negated() {
      GlobPattern {
        base_dir: self.base_dir,
        relative_pattern: self.relative_pattern[1..].to_string(),
      }
    } else {
      self
    }
  }

  pub fn into_negated(self) -> GlobPattern {
    if self.is_negated() {
      self
    } else {
      GlobPattern {
        base_dir: self.base_dir,
        relative_pattern: format!("!{}", self.relative_pattern),
      }
    }
  }

  pub fn into_new_base(self, new_base_dir: PathBuf) -> Self {
    assert!(self.base_dir.starts_with(&new_base_dir));
    if self.base_dir == new_base_dir {
      self
    } else {
      let is_negated = self.is_negated();

      let start_pattern = {
        let mut value = self
          .base_dir
          .strip_prefix(&new_base_dir)
          .unwrap()
          .to_string_lossy()
          .to_string()
          .replace("\\", "/");
        if value.starts_with("./") {
          value.drain(..2);
        }
        if value.starts_with("/") {
          value.drain(..1);
        }
        value
      };

      let new_relative_pattern = {
        let mut value = self.relative_pattern;
        if is_negated {
          value.drain(..1); // remove !
        }
        if !value.contains("/") {
          // patterns without a slash should match every directory
          value = format!("**/{}", value);
        } else if value.starts_with("./") {
          value.drain(..2);
        } else if value.starts_with("/") {
          value.drain(..1);
        }
        value
      };

      let new_pattern = {
        let mut value = String::new();
        if is_negated {
          value.push_str("!");
        }
        value.push_str("./");
        if !start_pattern.is_empty() {
          value.push_str(&start_pattern);
          value.push_str("/");
        }
        value.push_str(&new_relative_pattern);
        value
      };
      GlobPattern::new(new_pattern, new_base_dir)
    }
  }
}

#[derive(Debug)]
pub struct GlobPatterns {
  pub includes: Vec<GlobPattern>,
  pub excludes: Vec<GlobPattern>,
}

#[cfg(test)]
mod test {
  use std::path::PathBuf;

  use super::*;

  #[test]
  fn should_make_negated() {
    let pattern = GlobPattern::new("**/*".to_string(), PathBuf::from("/test")).into_negated();
    assert_eq!(pattern.relative_pattern, "!**/*");
    assert_eq!(pattern.absolute_pattern(), "!/test/**/*");

    // should keep as-is
    let pattern = GlobPattern::new("!**/*".to_string(), PathBuf::from("/test")).into_negated();
    assert_eq!(pattern.relative_pattern, "!**/*");
    assert_eq!(pattern.absolute_pattern(), "!/test/**/*");
  }

  #[test]
  fn should_make_non_negated() {
    let pattern = GlobPattern::new("!**/*".to_string(), PathBuf::from("/test")).into_non_negated();
    assert_eq!(pattern.relative_pattern, "**/*");
    assert_eq!(pattern.absolute_pattern(), "/test/**/*");

    // should keep as-is
    let pattern = GlobPattern::new("**/*".to_string(), PathBuf::from("/test")).into_non_negated();
    assert_eq!(pattern.relative_pattern, "**/*");
    assert_eq!(pattern.absolute_pattern(), "/test/**/*");
  }

  #[test]
  fn should_make_with_new_base() {
    let pattern = GlobPattern::new("**/*".to_string(), PathBuf::from("/test/dir"));
    assert_eq!(pattern.relative_pattern, "**/*");
    assert_eq!(pattern.base_dir, PathBuf::from("/test/dir"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/*");

    let pattern = pattern.into_new_base(PathBuf::from("/test"));
    assert_eq!(pattern.relative_pattern, "./dir/**/*");
    assert_eq!(pattern.base_dir, PathBuf::from("/test"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/*");
  }

  #[test]
  fn should_make_with_new_base_when_relative() {
    let pattern = GlobPattern::new("./**/*".to_string(), PathBuf::from("/test/dir"));
    let pattern = pattern.into_new_base(PathBuf::from("/"));
    assert_eq!(pattern.relative_pattern, "./test/dir/**/*");
    assert_eq!(pattern.base_dir, PathBuf::from("/"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/*");
  }

  #[test]
  fn should_make_new_base_when_no_slash() {
    let pattern = GlobPattern::new("asdf".to_string(), PathBuf::from("/test/dir"));
    assert_eq!(pattern.relative_pattern, "asdf");
    assert_eq!(pattern.base_dir, PathBuf::from("/test/dir"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/asdf");

    let pattern = pattern.into_new_base(PathBuf::from("/test"));
    assert_eq!(pattern.relative_pattern, "./dir/**/asdf");
    assert_eq!(pattern.base_dir, PathBuf::from("/test"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/asdf");

    let pattern = pattern.into_new_base(PathBuf::from("/"));
    assert_eq!(pattern.relative_pattern, "./test/dir/**/asdf");
    assert_eq!(pattern.base_dir, PathBuf::from("/"));
    assert_eq!(pattern.absolute_pattern(), "/test/dir/**/asdf");
  }
}
