use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use crate::environment::Environment;

/// Lifted from the Deno code.
/// Copyright 2018-2024 the Deno authors. MIT license.
/// Resolved gitignore for a directory.
pub struct DirGitIgnores {
  current: Option<Rc<ignore::gitignore::Gitignore>>,
  parent: Option<Rc<DirGitIgnores>>,
}

impl DirGitIgnores {
  pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
    let mut is_ignored = false;
    if let Some(parent) = &self.parent {
      is_ignored = parent.is_ignored(path, is_dir);
    }
    if let Some(current) = &self.current {
      match current.matched(path, is_dir) {
        ignore::Match::None => {}
        ignore::Match::Ignore(_) => {
          is_ignored = true;
        }
        ignore::Match::Whitelist(_) => {
          is_ignored = false;
        }
      }
    }
    is_ignored
  }
}

/// Resolves the lines of git's global excludes file when global gitignore
/// support is opted into via the `DPRINT_GLOBAL_GITIGNORE` environment variable.
/// Returns an empty list when disabled or when no global excludes file exists.
pub fn resolve_global_gitignore_lines(environment: &impl Environment) -> Vec<String> {
  if !global_gitignore_enabled(environment) {
    return Vec::new();
  }
  let Some(path) = environment.global_gitignore_path() else {
    return Vec::new();
  };
  match environment.maybe_read_file(&path) {
    Ok(Some(text)) => text.lines().map(|line| line.to_string()).collect(),
    Ok(None) => Vec::new(), // no global excludes file present
    Err(err) => {
      log_debug!(environment, "Failed reading global gitignore '{}': {:#}", path.display(), err);
      Vec::new()
    }
  }
}

fn global_gitignore_enabled(environment: &impl Environment) -> bool {
  match environment.env_var("DPRINT_GLOBAL_GITIGNORE") {
    Some(value) => {
      let value = value.to_string_lossy();
      let value = value.trim();
      value == "1" || value.eq_ignore_ascii_case("true")
    }
    None => false,
  }
}

/// Whether `.gitignore` and `.git` are present in a directory the caller has
/// already listed. Providing this lets resolution skip file system calls for
/// files it can already tell are absent.
#[derive(Clone, Copy)]
pub struct DirEntriesHint {
  pub has_gitignore: bool,
  pub has_git: bool,
}

#[derive(Default)]
pub struct GitIgnoreTreeOptions {
  /// Paths that should override what's in the gitignore.
  pub include_paths: Vec<PathBuf>,
  /// Lines from git's global excludes file, applied at the repository root with
  /// the lowest precedence. Empty unless global gitignore support is opted into.
  pub global_gitignore_lines: Vec<String>,
}

/// Resolves gitignores in a directory tree taking into account
/// ancestor gitignores that may be found in a directory.
pub struct GitIgnoreTree<TEnvironment> {
  environment: TEnvironment,
  ignores: HashMap<PathBuf, Option<Rc<DirGitIgnores>>>,
  options: GitIgnoreTreeOptions,
}

impl<TEnvironment: Environment> GitIgnoreTree<TEnvironment> {
  pub fn new(environment: TEnvironment, options: GitIgnoreTreeOptions) -> Self {
    Self {
      environment,
      ignores: Default::default(),
      options,
    }
  }

  /// Resolves the gitignore for the children of a directory the caller has
  /// already listed, passing a hint so resolution can avoid redundant reads.
  pub fn get_resolved_git_ignore_for_dir_children(&mut self, dir_path: &Path, hint: DirEntriesHint) -> Option<Rc<DirGitIgnores>> {
    self.get_resolved_git_ignore_inner(dir_path, Some(hint))
  }

  pub fn get_resolved_git_ignore_for_file(&mut self, file_path: &Path) -> Option<Rc<DirGitIgnores>> {
    let dir_path = file_path.parent()?;
    self.get_resolved_git_ignore_inner(dir_path, None)
  }

  fn get_resolved_git_ignore_inner(&mut self, dir_path: &Path, hint: Option<DirEntriesHint>) -> Option<Rc<DirGitIgnores>> {
    let maybe_resolved = self.ignores.get(dir_path).cloned();
    if let Some(resolved) = maybe_resolved {
      resolved
    } else {
      let resolved = self.resolve_gitignore_in_dir(dir_path, hint);
      self.ignores.insert(dir_path.to_owned(), resolved.clone());
      resolved
    }
  }

  fn resolve_gitignore_in_dir(&mut self, dir_path: &Path, hint: Option<DirEntriesHint>) -> Option<Rc<DirGitIgnores>> {
    // a directory containing `.git` is the root of a repository, so don't
    // search for gitignores above it
    let is_repo_root = match hint {
      Some(hint) => hint.has_git,
      None => self.environment.path_exists(dir_path.join(".git")),
    };
    let parent = if is_repo_root {
      None
    } else {
      // ancestors aren't part of the caller's listing, so resolve them without a hint
      dir_path.parent().and_then(|parent| self.get_resolved_git_ignore_inner(parent, None))
    };
    let current = self.resolve_current_gitignore(dir_path, is_repo_root, hint);
    if parent.is_none() && current.is_none() {
      None
    } else {
      Some(Rc::new(DirGitIgnores { current, parent }))
    }
  }

  fn resolve_current_gitignore(&self, dir_path: &Path, is_repo_root: bool, hint: Option<DirEntriesHint>) -> Option<Rc<ignore::gitignore::Gitignore>> {
    // skip the read when the caller's listing already shows there's no `.gitignore`
    let maybe_has_gitignore = hint.map(|h| h.has_gitignore).unwrap_or(true);
    let gitignore_text = if maybe_has_gitignore {
      self.environment.read_file(dir_path.join(".gitignore")).ok()
    } else {
      None
    };
    // git also reads `.git/info/exclude` at the repository root, treating it
    // like an uncommitted `.gitignore` there (https://git-scm.com/docs/gitignore).
    // Only the repo root can have this file, so avoid the read everywhere else.
    let exclude_text = if is_repo_root {
      self.environment.read_file(dir_path.join(".git").join("info").join("exclude")).ok()
    } else {
      None
    };
    // git's global excludes file applies repository-wide, so resolve it at the
    // repo root where it becomes the parent of every descendant directory
    let global_lines: &[String] = if is_repo_root { self.options.global_gitignore_lines.as_slice() } else { &[] };
    if gitignore_text.is_none() && exclude_text.is_none() && global_lines.is_empty() {
      return None;
    }

    let mut builder = ignore::gitignore::GitignoreBuilder::new(dir_path);
    // git's precedence is global excludes < `.git/info/exclude` < `.gitignore`,
    // and the last matching pattern wins, so add them in that order
    for line in global_lines {
      builder.add_line(None, line).ok()?;
    }
    if let Some(text) = &exclude_text {
      for line in text.lines() {
        builder.add_line(None, line).ok()?;
      }
    }
    if let Some(text) = &gitignore_text {
      for line in text.lines() {
        builder.add_line(None, line).ok()?;
      }
    }
    // override the gitignore contents to include these paths
    for path in &self.options.include_paths {
      if let Ok(suffix) = path.strip_prefix(dir_path) {
        let suffix = suffix.to_string_lossy().replace('\\', "/");
        let _ignore = builder.add_line(None, &format!("!/{}", suffix));
        if !suffix.ends_with('/') {
          let _ignore = builder.add_line(None, &format!("!/{}/", suffix));
        }
      }
    }
    let gitignore = builder.build().ok()?;
    Some(Rc::new(gitignore))
  }
}

#[cfg(test)]
mod test {
  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn git_ignore_tree() {
    let env = TestEnvironment::new();
    env.write_file("/.gitignore", "file.txt").unwrap();
    env.mk_dir_all("/sub_dir/sub_dir").unwrap();
    env.write_file("/sub_dir/.gitignore", "data.txt").unwrap();
    env.write_file("/sub_dir/sub_dir/.gitignore", "!file.txt\nignore.txt").unwrap();
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    let mut run_test = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
      assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
    };
    run_test("/file.txt", true);
    run_test("/other.txt", false);
    run_test("/data.txt", false);
    run_test("/sub_dir/file.txt", true);
    run_test("/sub_dir/other.txt", false);
    run_test("/sub_dir/data.txt", true);
    run_test("/sub_dir/sub_dir/file.txt", false); // unignored up here
    run_test("/sub_dir/sub_dir/sub_dir/file.txt", false);
    run_test("/sub_dir/sub_dir/sub_dir/ignore.txt", true);
    run_test("/sub_dir/sub_dir/ignore.txt", true);
    run_test("/sub_dir/ignore.txt", false);
    run_test("/ignore.txt", false);
  }

  #[test]
  fn honours_git_info_exclude() {
    let env = TestEnvironment::new();
    env.write_file("/.gitignore", "from_gitignore.txt").unwrap();
    env.mk_dir_all("/.git/info").unwrap();
    env.write_file("/.git/info/exclude", "from_exclude.txt\n!unexclude.txt").unwrap();
    env.mk_dir_all("/sub_dir").unwrap();
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    let mut run_test = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
      assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
    };
    run_test("/from_gitignore.txt", true);
    run_test("/from_exclude.txt", true);
    // patterns in `.git/info/exclude` apply to descendant directories too
    run_test("/sub_dir/from_exclude.txt", true);
    run_test("/other.txt", false);
  }

  #[test]
  fn global_gitignore_is_lowest_precedence() {
    let env = TestEnvironment::new();
    // a `.git` dir makes `/` the repo root, where the global excludes apply
    env.mk_dir_all("/.git").unwrap();
    env.write_file("/.git/HEAD", "").unwrap();
    env.write_file("/.gitignore", "from_gitignore.txt\n!from_global.txt").unwrap();
    env.mk_dir_all("/sub").unwrap();
    let global_gitignore_lines = vec!["from_global.txt".to_string(), "*.log".to_string()];
    let mut ignore_tree = GitIgnoreTree::new(
      env,
      GitIgnoreTreeOptions {
        global_gitignore_lines,
        ..Default::default()
      },
    );
    let mut run_test = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
      assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
    };
    // ignored by the global excludes file
    run_test("/from_global.txt", false); // re-included by the closer `.gitignore`
    run_test("/debug.log", true); // global pattern, applies to descendants too
    run_test("/sub/debug.log", true);
    // ignored by the repo `.gitignore`
    run_test("/from_gitignore.txt", true);
    run_test("/other.txt", false);
  }

  #[test]
  fn git_info_exclude_without_gitignore() {
    // a repo with only `.git/info/exclude` and no `.gitignore` should still be honoured
    let env = TestEnvironment::new();
    env.mk_dir_all("/.git/info").unwrap();
    env.write_file("/.git/info/exclude", "ignored.txt").unwrap();
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    let path = PathBuf::from("/ignored.txt");
    let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
    assert!(gitignore.is_ignored(&path, /* is_dir */ false));
  }
}
