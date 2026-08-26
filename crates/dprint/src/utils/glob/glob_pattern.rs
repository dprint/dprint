use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;

use crate::environment::CanonicalizedPathBuf;

use super::escape_glob_text;
use super::is_negated_glob;
use super::is_pattern;
use super::non_negated_glob;
use super::pattern_names_dir;
use super::unescape_glob_text;

/// What a pattern does with the files it matches, which decides whether naming
/// a directory also covers everything within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobPatternKind {
  /// Naming a directory only matches that path (ex. a config `includes` entry
  /// of `sub` doesn't pull in `sub/file.ts`—that needs `sub/**`).
  Include,
  /// Naming a directory excludes everything within it, the same way gitignore
  /// and a traversal that prunes the directory behave.
  Exclude,
}

#[derive(Debug)]
pub struct GlobPatterns {
  pub arg_includes: Option<Vec<GlobPattern>>,
  pub config_includes: Option<Vec<GlobPattern>>,
  pub arg_excludes: Option<Vec<GlobPattern>>,
  pub config_excludes: Vec<GlobPattern>,
  /// Whether to also match files without an extension regardless of the
  /// config includes (used to discover shebang scripts).
  pub include_extensionless_files: bool,
}

impl GlobPatterns {
  /// Resolves the include paths (not patterns).
  pub fn include_paths(&self) -> Vec<PathBuf> {
    // we only make the explicitly specified paths override the gitignore
    // because it starts getting really complicated with globs and some
    // people may not want globs to not match gitignored files
    self
      .arg_includes
      .iter()
      .flat_map(|i| i.iter())
      .chain(self.config_includes.iter().flat_map(|i| i.iter()))
      .filter_map(|pattern| {
        if !is_pattern(&pattern.relative_pattern) && !pattern.is_negated() {
          // unescape so an escaped literal (ex. `./\[id\].svelte`) resolves
          // to the actual file path
          Some(pattern.base_dir.join(unescape_glob_text(&pattern.relative_pattern).as_ref()))
        } else {
          None
        }
      })
      .collect()
  }
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

  pub fn matches_dir_for_traversal(&self, dir_path: &Path) -> bool {
    if self.is_negated() {
      return false;
    }

    if self.base_dir.as_ref().starts_with(dir_path) {
      // we're in an ancestor directory, so yes
      true
    } else if let Ok(remaining) = dir_path.strip_prefix(&self.base_dir) {
      // we're in a subdir, so start looking at the pattern
      let pattern = self.relative_pattern.strip_prefix("./").unwrap_or(&self.relative_pattern);
      let mut components = remaining.components().peekable();
      let mut parts = pattern.split('/').peekable();
      while let (Some(component), Some(part)) = (components.peek(), parts.peek()) {
        // this is intended to be simple and quick... it will overmatch at the moment
        // for patterns, which is fine
        let part = *part;
        let component = *component;
        if part == "**" {
          return true;
        } else if !is_pattern(part) && !component.as_os_str().eq_ignore_ascii_case(unescape_glob_text(part).as_ref()) {
          return false;
        }
        components.next();
        parts.next();
      }
      parts.next().is_some()
    } else {
      false
    }
  }

  pub fn is_negated(&self) -> bool {
    is_negated_glob(&self.relative_pattern)
  }

  pub fn invert(self) -> Self {
    if self.is_negated() {
      GlobPattern {
        base_dir: self.base_dir,
        relative_pattern: non_negated_glob(&self.relative_pattern).to_string(),
      }
    } else {
      GlobPattern {
        base_dir: self.base_dir,
        relative_pattern: format!("!{}", self.relative_pattern),
      }
    }
  }

  /// Converts the pattern to have a base directory path that goes as
  /// deep as it can until it hits a pattern component or the last component
  /// which is a possible file name.
  pub fn into_deepest_base(self) -> Self {
    let is_negated = self.is_negated();
    let pattern = non_negated_glob(&self.relative_pattern);
    let stripped_dot_slash = pattern.starts_with("./");
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let parts: Vec<&str> = pattern.split('/').collect();

    let mut base_parts = Vec::new();
    let mut remaining_parts = Vec::new();
    let mut found_glob = false;

    for part in &parts {
      if !found_glob && !is_pattern(part) {
        base_parts.push(*part);
      } else {
        found_glob = true;
        remaining_parts.push(*part);
      }
    }

    // handle case where there are no globs (treat last segment as pattern)
    if !found_glob && !base_parts.is_empty() {
      remaining_parts.push(base_parts.pop().unwrap());
    }

    let new_base_dir = if base_parts.is_empty() {
      self.base_dir.clone()
    } else {
      // unescape so escaped literal parts (ex. `\[a\]`) become the actual
      // directory name in the new base path
      let base_parts = base_parts.iter().map(|part| unescape_glob_text(part)).collect::<Vec<_>>();
      self.base_dir.join_panic_relative(base_parts.join("/"))
    };

    let new_relative = remaining_parts.join("/");
    let new_relative = if stripped_dot_slash { format!("./{}", new_relative) } else { new_relative };
    let new_pattern = if is_negated { format!("!{}", new_relative) } else { new_relative };

    GlobPattern {
      base_dir: new_base_dir,
      relative_pattern: new_pattern,
    }
  }

  pub fn into_new_base(self, new_base_dir: CanonicalizedPathBuf, kind: GlobPatternKind) -> Option<Self> {
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
        // escape so glob characters in the path (ex. a `[a]` directory)
        // match literally
        escape_glob_text(&value)
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
      let is_negated = self.is_negated();
      let non_negated = non_negated_glob(&self.relative_pattern);
      // a ./ prefix anchors the pattern to its base directory, so strip it
      // before walking the prefix and keep the result anchored to the new base
      let is_anchored = non_negated.starts_with("./");
      let mut pattern = non_negated.strip_prefix("./").unwrap_or(non_negated);
      let build_pattern = |pattern: &str| {
        let mut value = String::new();
        if is_negated {
          value.push('!');
        }
        if is_anchored {
          value.push_str("./");
        }
        value.push_str(pattern);
        value
      };
      let prefix = prefix.to_string_lossy();
      let mut prefix = prefix
        .split(if cfg!(windows) { if prefix.contains('\\') { '\\' } else { '/' } } else { '/' })
        .collect::<VecDeque<_>>();

      // an exclude naming a directory covers everything within it, so a
      // pattern that names an ancestor of the new base becomes `**`. This
      // doesn't apply to includes, where naming a directory only matches that
      // one path.
      let covers_new_base = kind == GlobPatternKind::Exclude;

      // an empty pattern (ex. `--excludes ..` resolving to its base directory)
      // names the old base directory, an ancestor of the new base
      if pattern.is_empty() {
        return covers_new_base.then(|| GlobPattern::new(build_pattern("**"), new_base_dir));
      }

      // A pattern that isn't anchored and has no internal slash (ex. `dist`,
      // `*.min.js`, `dist/`) matches its name at any depth below the base
      // directory, so it matches at any depth below the new base too and
      // survives the rebase unchanged. It never consumes prefix components,
      // so the loop below would find no sub match and incorrectly drop it.
      if !is_anchored && !pattern.trim_end_matches('/').contains('/') {
        // an exclude naming any directory between the old and new base means
        // the new base sits inside an excluded directory, which covers
        // everything within it
        if covers_new_base && names_ancestor_of_new_base(pattern.trim_end_matches('/'), &prefix) {
          return Some(GlobPattern::new(build_pattern("**"), new_base_dir));
        }
        return Some(GlobPattern::new(build_pattern(pattern), new_base_dir));
      }

      loop {
        let mut found_sub_match = false;
        if pattern == "**" || pattern.starts_with("**/") {
          // a `**/name` pattern floats by depth just like a slashless one, so it
          // also covers the new base when it names one of the directories above it
          if covers_new_base
            && let Some(name) = pattern.strip_prefix("**/")
            && !name.trim_end_matches('/').contains('/')
            && names_ancestor_of_new_base(name.trim_end_matches('/'), &prefix)
          {
            return Some(GlobPattern::new(build_pattern("**"), new_base_dir));
          }
          return Some(GlobPattern::new(build_pattern(pattern), new_base_dir));
        }
        // a final pattern component naming this directory means the pattern
        // covers the new base directory, so it applies to everything within
        // (ex. an `--excludes ../other` arg rebased into a scope at `other`)
        if covers_new_base
          && !pattern.contains('/')
          && let Some(first_item) = prefix.front()
          && pattern_names_dir(pattern, first_item)
        {
          return Some(GlobPattern::new(build_pattern("**"), new_base_dir));
        }
        // check for a * dir
        if let Some(new_pattern) = pattern.strip_prefix("*/") {
          pattern = new_pattern;
          prefix.pop_front();
          if prefix.is_empty() {
            // we've hit the new base directory
            return Some(GlobPattern::new(build_pattern(pattern), new_base_dir));
          }
          found_sub_match = true;
        }
        // check for a match for the name (an escaped literal part like `\[a\]`
        // matches the actual directory name, and a wildcard part like `su*`
        // matches what it would match when the pattern is finally matched)
        let first_item = prefix.front().unwrap();
        if let Some((first_part, new_pattern)) = pattern.split_once('/')
          && pattern_names_dir(first_part, first_item)
        {
          pattern = new_pattern;
          prefix.pop_front();
          if prefix.is_empty() {
            // we've hit the new base directory
            return Some(GlobPattern::new(build_pattern(pattern), new_base_dir));
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

  pub fn as_absolute_pattern_text(&self) -> String {
    let is_negated = self.is_negated();
    let pattern = non_negated_glob(&self.relative_pattern);
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let mut base = self.base_dir.to_string_lossy().to_string();
    if cfg!(windows) {
      base = base.replace("\\", "/");
    }
    if !base.ends_with("/") && !pattern.starts_with("/") {
      base.push('/');
    }
    base.push_str(pattern);
    if is_negated {
      base = format!("!{}", base);
    }
    base
  }
}

/// Whether a single component pattern names any of the directories between the
/// pattern's base directory and the new base, which for an exclude means the
/// new base sits inside an excluded directory.
fn names_ancestor_of_new_base(name: &str, prefix: &VecDeque<&str>) -> bool {
  prefix.iter().any(|part| pattern_names_dir(name, part))
}

#[cfg(test)]
mod test {
  use super::*;

  /// Rebases the pattern, asserting both kinds agree. The cases where include
  /// and exclude semantics differ are asserted with explicit kinds instead.
  #[track_caller]
  fn into_new_base(pattern: GlobPattern, new_base_dir: CanonicalizedPathBuf) -> Option<GlobPattern> {
    let as_include = pattern.clone().into_new_base(new_base_dir.clone(), GlobPatternKind::Include);
    let as_exclude = pattern.into_new_base(new_base_dir, GlobPatternKind::Exclude);
    assert_eq!(as_include, as_exclude);
    as_exclude
  }

  #[test]
  fn should_invert() {
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let pattern = GlobPattern::new("!**/*".to_string(), test_dir.clone()).invert();
    assert_eq!(pattern.relative_pattern, "**/*");

    // should keep as-is
    let pattern = GlobPattern::new("**/*".to_string(), test_dir).invert();
    assert_eq!(pattern.relative_pattern, "!**/*");
  }

  #[test]
  fn should_make_with_new_base() {
    let test_dir = CanonicalizedPathBuf::new_for_testing("/test");
    let test_dir_dir = CanonicalizedPathBuf::new_for_testing("/test/dir");
    let pattern = GlobPattern::new("**/*".to_string(), test_dir_dir.clone());
    assert_eq!(pattern.relative_pattern, "**/*");
    assert_eq!(pattern.base_dir, test_dir_dir);

    let pattern = into_new_base(pattern, test_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./dir/**/*");
    assert_eq!(pattern.base_dir, test_dir);
  }

  #[test]
  fn should_make_with_new_base_when_relative() {
    let root_dir = CanonicalizedPathBuf::new_for_testing("/");
    let test_dir_dir = CanonicalizedPathBuf::new_for_testing("/test/dir");
    let pattern = GlobPattern::new("./**/*".to_string(), test_dir_dir);
    let pattern = into_new_base(pattern, root_dir.clone()).unwrap();
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

    let pattern = into_new_base(pattern, test_dir.clone()).unwrap();
    assert_eq!(pattern.relative_pattern, "./dir/**/asdf");
    assert_eq!(pattern.base_dir, test_dir);

    let pattern = into_new_base(pattern, root_dir.clone()).unwrap();
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
    assert_eq!(into_new_base(pattern, sibling_dir.clone()), None);
  }

  #[test]
  fn should_handle_mapping_into_parent_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("**/*.ts".to_string(), base_dir.clone());
    let parent_dir = CanonicalizedPathBuf::new_for_testing("/");
    let new_pattern = into_new_base(pattern, parent_dir.clone()).unwrap();
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
      let new_pattern = into_new_base(pattern.clone(), child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "**/*.ts");
    }
    // grandchild
    {
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      let new_pattern = into_new_base(pattern, grandchild_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, grandchild_dir);
      assert_eq!(new_pattern.relative_pattern, "**/*.ts");
    }
    // negated
    {
      let pattern = GlobPattern::new("!**/*.ts".to_string(), base_dir.clone());
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      let new_pattern = into_new_base(pattern, grandchild_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, grandchild_dir);
      assert_eq!(new_pattern.relative_pattern, "!**/*.ts");
    }
  }

  /// An exclude naming a directory covers everything within it, so it becomes
  /// `**` in the new base. An include naming a directory only matches that one
  /// path, so it has nothing to say about the directory's contents and is
  /// dropped instead.
  #[test]
  fn should_handle_mapping_into_descendant_dir_the_pattern_names() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    // the pattern naming the new base directory
    {
      let pattern = GlobPattern::new("./sub".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "./**");
      assert_eq!(pattern.into_new_base(child_dir, GlobPatternKind::Include), None);
    }
    // same for an ancestor of the new base directory
    {
      let pattern = GlobPattern::new("./sub".to_string(), base_dir.clone());
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      let new_pattern = pattern.clone().into_new_base(grandchild_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.base_dir, grandchild_dir);
      assert_eq!(new_pattern.relative_pattern, "./**");
      assert_eq!(pattern.into_new_base(grandchild_dir, GlobPatternKind::Include), None);
    }
    // negated
    {
      let pattern = GlobPattern::new("!./sub".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.into_new_base(child_dir, GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "!./**");
    }
    // escaped glob characters in the name
    {
      let pattern = GlobPattern::new("./\\[sub\\]".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/[sub]");
      let new_pattern = pattern.into_new_base(child_dir, GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "./**");
    }
    // a * final component matches any directory name
    {
      let pattern = GlobPattern::new("./*".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "./**");
      // as an include, `./*` matches the directory but not what's inside it
      assert_eq!(pattern.into_new_base(child_dir, GlobPatternKind::Include), None);
    }
    // an empty pattern names the old base directory itself
    // (ex. `--excludes ..` resolving to its base directory)
    {
      let pattern = GlobPattern::new("./".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = pattern.clone().into_new_base(child_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "./**");
      assert_eq!(pattern.into_new_base(child_dir, GlobPatternKind::Include), None);
    }
  }

  /// A pattern that isn't anchored and has no slash matches its name at any
  /// depth, so it survives being rebased into a descendant directory.
  #[test]
  fn should_handle_mapping_depth_floating_pattern_into_descendant_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
    let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/nested");
    // kept as-is no matter how deep the new base is
    for pattern_text in ["dist", "*.min.js", "dist/", "!dist"] {
      for new_base_dir in [&child_dir, &descendant_dir] {
        let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
        let new_pattern = into_new_base(pattern, new_base_dir.clone()).unwrap();
        assert_eq!(new_pattern.base_dir, *new_base_dir);
        assert_eq!(new_pattern.relative_pattern, pattern_text);
      }
    }
    // an exclude naming a directory the new base is within covers everything in it
    for pattern_text in ["sub", "sub/", "*", "\\[sub\\]"] {
      let new_base_dir = if pattern_text.starts_with('\\') {
        CanonicalizedPathBuf::new_for_testing("/base/[sub]/nested")
      } else {
        descendant_dir.clone()
      };
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.clone().into_new_base(new_base_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.base_dir, new_base_dir);
      assert_eq!(new_pattern.relative_pattern, "**");
      // an include naming a directory says nothing about its contents, so it
      // stays a name match at any depth below the new base
      let new_pattern = pattern.into_new_base(new_base_dir, GlobPatternKind::Include).unwrap();
      assert_eq!(new_pattern.relative_pattern, pattern_text);
    }
    // ...including when the name is a wildcard that matches the directory
    for pattern_text in ["su*", "?ub", "[sd]ub", "s{ub,omething}"] {
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(descendant_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "**");
    }
    // ...and when the name is one of the directories above the new base rather
    // than the first one below the old base
    for pattern_text in ["nested", "neste*"] {
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(descendant_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, "**");
    }
    // a wildcard that doesn't match any of them keeps floating by depth
    for pattern_text in ["ot*", "[abc]ub"] {
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(descendant_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, pattern_text);
    }
    // an anchored pattern is unaffected (it only matches at its own base)
    {
      let pattern = GlobPattern::new("./dist".to_string(), base_dir.clone());
      assert_eq!(pattern.into_new_base(child_dir, GlobPatternKind::Include), None);
    }
  }

  /// A `**/name` pattern floats by depth the same way a slashless one does, so
  /// it also covers a new base that sits inside a directory it names.
  #[test]
  fn should_handle_mapping_depth_floating_glob_star_pattern_into_descendant_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let inside_dir = CanonicalizedPathBuf::new_for_testing("/base/node_modules/pkg");
    // an exclude whose name is a directory the new base is within covers it all
    for pattern_text in ["**/node_modules", "**/node_modules/", "**/node_module*"] {
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(inside_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.base_dir, inside_dir);
      assert_eq!(new_pattern.relative_pattern, "**");
    }
    // an include says nothing about the contents, so it keeps floating by depth
    {
      let pattern = GlobPattern::new("**/node_modules".to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(inside_dir.clone(), GlobPatternKind::Include).unwrap();
      assert_eq!(new_pattern.relative_pattern, "**/node_modules");
    }
    // one that names none of the directories above the new base is unaffected
    for pattern_text in ["**/dist", "**/node_modules/dist", "**"] {
      let pattern = GlobPattern::new(pattern_text.to_string(), base_dir.clone());
      let new_pattern = pattern.into_new_base(inside_dir.clone(), GlobPatternKind::Exclude).unwrap();
      assert_eq!(new_pattern.relative_pattern, pattern_text);
    }
  }

  #[test]
  fn should_handle_mapping_into_child_dir_if_star() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    let pattern = GlobPattern::new("*/*.ts".to_string(), base_dir.clone());
    // child
    {
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern.clone(), child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "*.ts");
    }
    // grandchild
    {
      let grandchild_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/dir");
      assert_eq!(into_new_base(pattern, grandchild_dir.clone()), None);
    }
    // negated
    {
      let pattern = GlobPattern::new("!*/*.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!*.ts");
    }
  }

  #[test]
  fn should_handle_mapping_anchored_pattern_into_descendant_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("./sub/a.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "./a.ts");
    }
    {
      let pattern = GlobPattern::new("!./sub/a.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!./a.ts");
    }
    {
      let pattern = GlobPattern::new("./sub/**/*.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "./**/*.ts");
    }
    {
      // deeper descendant
      let pattern = GlobPattern::new("./sub/nested/a.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/nested");
      let new_pattern = into_new_base(pattern, descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "./a.ts");
    }
    {
      // not under the new base directory
      let pattern = GlobPattern::new("./other/a.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      assert_eq!(into_new_base(pattern, child_dir), None);
    }
  }

  #[test]
  fn should_handle_mapping_bare_match_all_pattern_into_descendant_dir() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("**".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "**");
    }
    {
      let pattern = GlobPattern::new("./**".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "./**");
    }
    {
      // dir pattern expanded to dir/** rebased into a deeper descendant
      let pattern = GlobPattern::new("sub/**".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/nested");
      let new_pattern = into_new_base(pattern, descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "**");
    }
    {
      let pattern = GlobPattern::new("!**".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern, child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!**");
    }
  }

  #[test]
  fn should_handle_mapping_into_dir_if_pattern_matches_name() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("!sub/*.ts".to_string(), base_dir.clone());
      let child_dir = CanonicalizedPathBuf::new_for_testing("/base/sub");
      let new_pattern = into_new_base(pattern.clone(), child_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, child_dir);
      assert_eq!(new_pattern.relative_pattern, "!*.ts");
    }
    {
      let pattern = GlobPattern::new("sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/something/dir");
      let new_pattern = into_new_base(pattern.clone(), descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "*.ts");
    }
    {
      let pattern = GlobPattern::new("!sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("/base/sub/something");
      let new_pattern = into_new_base(pattern.clone(), descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "!dir/*.ts");
    }
    if cfg!(windows) {
      let base_dir = CanonicalizedPathBuf::new_for_testing("C:\\base");
      let pattern = GlobPattern::new("!sub/*/dir/*.ts".to_string(), base_dir.clone());
      let descendant_dir = CanonicalizedPathBuf::new_for_testing("C:\\base\\sub\\something");
      let new_pattern = into_new_base(pattern.clone(), descendant_dir.clone()).unwrap();
      assert_eq!(new_pattern.base_dir, descendant_dir);
      assert_eq!(new_pattern.relative_pattern, "!dir/*.ts");
    }
  }

  #[test]
  fn into_deepest_base() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("!sub/*.ts".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base/sub"));
      assert_eq!(new_pattern.relative_pattern, "!*.ts");
    }
    {
      let pattern = GlobPattern::new("sub/testing/this/**/out/*.ts".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base/sub/testing/this"));
      assert_eq!(new_pattern.relative_pattern, "**/out/*.ts");
    }
    {
      let pattern = GlobPattern::new("testing".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base"));
      assert_eq!(new_pattern.relative_pattern, "testing");
    }
    {
      let pattern = GlobPattern::new("sub/testing".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base/sub"));
      assert_eq!(new_pattern.relative_pattern, "testing");
    }
    {
      let pattern = GlobPattern::new("testing.js".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base"));
      assert_eq!(new_pattern.relative_pattern, "testing.js");
    }
    {
      let pattern = GlobPattern::new("./testing.js".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base"));
      assert_eq!(new_pattern.relative_pattern, "./testing.js");
    }
    {
      let pattern = GlobPattern::new("./sub/**/testing.js".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base/sub"));
      assert_eq!(new_pattern.relative_pattern, "./**/testing.js");
    }
    {
      let pattern = GlobPattern::new("!./sub/**/testing.js".to_string(), base_dir.clone());
      let new_pattern = pattern.into_deepest_base();
      assert_eq!(new_pattern.base_dir, CanonicalizedPathBuf::new_for_testing("/base/sub"));
      assert_eq!(new_pattern.relative_pattern, "!./**/testing.js");
    }
  }

  #[test]
  fn as_absolute_pattern_text() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("!sub/*.ts".to_string(), base_dir.clone());
      assert_eq!(pattern.as_absolute_pattern_text(), "!/base/sub/*.ts");
    }
    {
      let pattern = GlobPattern::new("testing/this/out/*.ts".to_string(), base_dir.clone());
      assert_eq!(pattern.as_absolute_pattern_text(), "/base/testing/this/out/*.ts");
    }
    {
      let base_dir = CanonicalizedPathBuf::new_for_testing("/base/");
      let pattern = GlobPattern::new("asdf".to_string(), base_dir);
      assert_eq!(pattern.as_absolute_pattern_text(), "/base/asdf");
    }
    {
      let pattern = GlobPattern::new("/asdf".to_string(), base_dir.clone());
      assert_eq!(pattern.as_absolute_pattern_text(), "/base/asdf");
    }
  }

  #[test]
  fn matches_dir_for_traversal() {
    let base_dir = CanonicalizedPathBuf::new_for_testing("/base");
    {
      let pattern = GlobPattern::new("sub/*.ts".to_string(), base_dir.clone());
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub")));
      assert!(!pattern.matches_dir_for_traversal(&base_dir.join("sub/test")));
      assert!(!pattern.matches_dir_for_traversal(&base_dir.join("sub/test/no")));
    }
    {
      let pattern = GlobPattern::new("sub/**/testing".to_string(), base_dir.clone());
      assert!(pattern.matches_dir_for_traversal(base_dir.as_ref()));
      assert!(!pattern.matches_dir_for_traversal(&base_dir.join("other")));
      // once a ** is hit it will match regardless
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/test")));
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/test/yes")));
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/test/yes/testing")));
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/test/yes/testing/testing/asdf")));
    }
    {
      let pattern = GlobPattern::new("sub/*/testing".to_string(), base_dir.clone());
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/test")));
      assert!(pattern.matches_dir_for_traversal(&base_dir.join("sub/other")));
      assert!(!pattern.matches_dir_for_traversal(&base_dir.join("sub/test/testing")));
      assert!(!pattern.matches_dir_for_traversal(&base_dir.join("sub/test/no")));
    }
  }
}
