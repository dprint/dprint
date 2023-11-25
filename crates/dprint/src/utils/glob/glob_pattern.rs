use std::collections::VecDeque;

use crate::environment::CanonicalizedPathBuf;

use super::is_negated_glob;
use super::non_negated_glob;

#[derive(Debug)]
pub struct GlobPatterns {
  pub includes: Option<Vec<GlobPattern>>,
  pub excludes: Vec<GlobPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern {
  pub relative_pattern: String,
  pub base_dir: CanonicalizedPathBuf,
}

impl GlobPattern {
  pub fn new(relative_pattern: String, base_dir: CanonicalizedPathBuf) -> Self {
    GlobPattern { relative_pattern, base_dir }
  }

  pub fn new_vec(relative_patterns: Vec<String>, base_dir: CanonicalizedPathBuf) -> Vec<Self> {
    relative_patterns
      .into_iter()
      .map(|relative_pattern| GlobPattern::new(relative_pattern, base_dir.clone()))
      .collect()
  }

  pub fn is_negated(&self) -> bool {
    is_negated_glob(&self.relative_pattern)
  }

  pub fn into_non_negated(self) -> GlobPattern {
    if self.is_negated() {
      GlobPattern {
        base_dir: self.base_dir,
        relative_pattern: non_negated_glob(&self.relative_pattern).to_string(),
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

  pub fn into_new_base(self, new_base_dir: CanonicalizedPathBuf) -> Option<Self> {
    if self.base_dir == new_base_dir {
      Some(self)
    } else if let Ok(prefix) = self.base_dir.strip_prefix(&new_base_dir) {
      let is_negated = self.is_negated();

      let start_pattern = {
        let mut value = prefix.to_string_lossy().to_string().replace('\\', "/");
        if value.starts_with("./") {
          value.drain(..2);
        }
        if value.starts_with('/') {
          value.drain(..1);
        }
        value
      };

      let new_relative_pattern = {
        let mut value = self.relative_pattern;
        if is_negated {
          value.drain(..1); // remove !
        }
        if !value.contains('/') {
          // patterns without a slash should match every directory
          value = format!("**/{}", value);
        } else if value.starts_with("./") {
          value.drain(..2);
        } else if value.starts_with('/') {
          value.drain(..1);
        }
        value
      };

      let new_pattern = {
        let mut value = String::new();
        if is_negated {
          value.push('!');
        }
        value.push_str("./");
        if !start_pattern.is_empty() {
          value.push_str(&start_pattern);
          value.push('/');
        }
        value.push_str(&new_relative_pattern);
        value
      };
      Some(GlobPattern::new(new_pattern, new_base_dir))
    } else if let Ok(prefix) = new_base_dir.strip_prefix(&self.base_dir) {
      let is_negated = is_negated_glob(&self.relative_pattern);
      let mut pattern = non_negated_glob(&self.relative_pattern);
      let prefix = prefix.to_string_lossy();
      let mut prefix = prefix
        .split(if cfg!(windows) {
          if prefix.contains('\\') {
            '\\'
          } else {
            '/'
          }
        } else {
          '/'
        })
        .collect::<VecDeque<_>>();

      loop {
        let mut found_sub_match = false;
        if pattern.starts_with("**/") {
          return Some(GlobPattern::new(
            if is_negated { format!("!{}", pattern) } else { pattern.to_string() },
            new_base_dir,
          ));
        }
        // check for a * dir
        if let Some(new_pattern) = pattern.strip_prefix("*/") {
          pattern = new_pattern;
          prefix.pop_front();
          if prefix.is_empty() {
            // we've hit the new base directory
            return Some(GlobPattern::new(
              if is_negated { format!("!{}", pattern) } else { pattern.to_string() },
              new_base_dir,
            ));
          }
          found_sub_match = true;
        }
        // check for a match for the name
        let first_item = prefix.front().unwrap();
        if let Some(new_pattern) = pattern.strip_prefix(&format!("{}/", first_item)) {
          pattern = new_pattern;
          prefix.pop_front();
          if prefix.is_empty() {
            // we've hit the new base directory
            return Some(GlobPattern::new(
              if is_negated { format!("!{}", pattern) } else { pattern.to_string() },
              new_base_dir,
            ));
          }
          found_sub_match = true;
        }

        if !found_sub_match {
          return None;
        }
      }
    } else {
      None
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_make_negated() {
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let pattern = GlobPattern::new("**/*".to_string(), test_dir.clone()).into_negated();
    assert_eq!(pattern.relative_pattern, "!**/*");

    // should keep as-is
    let pattern = GlobPattern::new("!**/*".to_string(), test_dir).into_negated();
    assert_eq!(pattern.relative_pattern, "!**/*");
  }

  #[test]
  fn should_make_non_negated() {
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let pattern = GlobPattern::new("!**/*".to_string(), test_dir.clone()).into_non_negated();
    assert_eq!(pattern.relative_pattern, "**/*");

    // should keep as-is
    let pattern = GlobPattern::new("**/*".to_string(), test_dir).into_non_negated();
    assert_eq!(pattern.relative_pattern, "**/*");
  }

  #[test]
  fn should_make_with_new_base() {
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let test_dir_dir = CanonicalizedPathBuf::new_for_testing("/test/dir");
    let pattern = GlobPattern::new("**/*".to_string(), test_dir_dir.clone());
    assert_eq!(pattern.relative_pattern, "**/*");
    assert_eq!(pattern.base_dir, test_dir_dir);

    let pattern = pattern.into_new_base(test_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./dir/**/*");
    assert_eq!(pattern.base_dir, test_dir);
  }

  #[test]
  fn should_make_with_new_base_when_relative() {
    let root_dir = CanonicalizedPathBuf::new_for_testing("/");
    let test_dir_dir = CanonicalizedPathBuf::new_for_testing("/test/dir");
    let pattern = GlobPattern::new("./**/*".to_string(), test_dir_dir);
    let pattern = pattern.into_new_base(root_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./test/dir/**/*");
    assert_eq!(pattern.base_dir, root_dir);
  }

  #[test]
  fn should_make_new_base_when_no_slash() {
    let test_dir_dir = CanonicalizedPathBuf::new_for_testing("/test/dir");
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let root_dir = CanonicalizedPathBuf::new_for_testing("/");
    let pattern = GlobPattern::new("asdf".to_string(), test_dir_dir.clone());
    assert_eq!(pattern.relative_pattern, "asdf");
    assert_eq!(pattern.base_dir, test_dir_dir);

    let pattern = pattern.into_new_base(test_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./dir/**/asdf");
    assert_eq!(pattern.base_dir, test_dir);

    let pattern = pattern.into_new_base(root_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./test/dir/**/asdf");
    assert_eq!(pattern.base_dir, root_dir);
  }

  #[test]
  fn should_handle_mapping_into_base_that_is_not_base() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("asdf".to_string(), base_dir.clone());
    assert_eq!(pattern.relative_pattern, "asdf");
    assert_eq!(pattern.base_dir, base_dir);

    let sibling_dir = CanonicalizedPathBuf::new_for_testing("/sibling");
    assert_eq!(pattern.into_new_base(sibling_dir.clone()), None);
  }

  #[test]
  fn should_handle_mapping_into_parent_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("**/*.ts".to_string(), base_dir.clone());
    let parent_dir = CanonicalizedPathBuf::new_for_testing("/");
    let new_pattern = pattern.into_new_base(parent_dir.clone()).unwrap();
    assert_eq!(new_pattern.base_dir, parent_dir);
    assert_eq!(new_pattern.relative_pattern, "./base/**/*.ts");
  }

  #[test]
  fn should_handle_mapping_into_descendant_dir_if_star_star() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("**/*.ts".to_string(), base_dir.clone());
    // child
    {
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "**/*.ts");
    }
    // grandchild
    {
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      let new_pattern = pattern.into_new_base(grandchild_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, grandchild_dir);
      assert_eq!(new_pattern.relative_pattern, "**/*.ts");
    }
    // negated
    {
      let pattern = GlobPattern::new("!**/*.ts".to_string(), base_dir.clone());
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      let new_pattern = pattern.into_new_base(grandchild_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, grandchild_dir);
      assert_eq!(new_pattern.relative_pattern, "!**/*.ts");
    }
  }

  #[test]
  fn should_handle_mapping_into_child_dir_if_star() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("*/*.ts".to_string(), base_dir.clone());
    // child
    {
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "*.ts");
    }
    // grandchild
    {
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      assert_eq!(pattern.into_new_base(grandchild_dir.clone()), None);
    }
    // negated
    {
      let pattern = GlobPattern::new("!*/*.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.into_new_base(child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!*.ts");
    }
  }

  #[test]
  fn should_handle_mapping_into_dir_if_pattern_matches_name() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("!sub/*.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!*.ts");
    }
    {
      let pattern = GlobPattern::new("sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/something/dir");
      let new_pattern = pattern.clone().into_new_base(descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "*.ts");
    }
    {
      let pattern = GlobPattern::new("!sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/something");
      let new_pattern = pattern.clone().into_new_base(descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "!dir/*.ts");
    }
    if cfg!(windows) {
      let base_dir = CanonicalizedPathBuf::new_for_testing("C:\\base");
      let pattern = GlobPattern::new("!sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("C:\\base\\sub\\something");
      let new_pattern = pattern.clone().into_new_base(descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "!dir/*.ts");
    }
  }
}
