use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;

use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::utils::escape_glob_text;

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
#[derive(Clone, Copy, Default)]
pub struct DirEntriesHint {
  pub has_gitignore: bool,
  pub has_git: bool,
}

impl DirEntriesHint {
  /// Derives a hint from an already-read directory listing.
  pub fn from_dir_entries(entries: &[DirEntry]) -> Self {
    let mut hint = DirEntriesHint {
      has_git: false,
      has_gitignore: false,
    };
    for entry in entries {
      // `.gitignore` is a file and `.git` is usually a directory (a file in worktrees)
      let (DirEntry::Directory(path) | DirEntry::File { path }) = entry;
      match path.file_name().and_then(|f| f.to_str()) {
        Some(".gitignore") => hint.has_gitignore = true,
        Some(".git") => hint.has_git = true,
        _ => continue,
      }
      if hint.has_gitignore && hint.has_git {
        break; // nothing left to learn
      }
    }
    hint
  }
}

#[derive(Default)]
pub struct GitIgnoreTreeOptions {
  /// Paths that should override what's in the gitignore.
  pub include_paths: Vec<PathBuf>,
  /// Lines from git's global excludes file, applied at the repository root with
  /// the lowest precedence. Empty unless global gitignore support is opted into.
  pub global_gitignore_lines: Vec<String>,
  /// Whether to remember the state of the files each directory's gitignore was
  /// resolved from so `refresh_for_file` can detect when they change. This is
  /// for a tree that outlives a single run (ex. in the language server) and
  /// it's off by default because of what it costs: a stat of `.git` and of
  /// `.gitignore` for a directory, plus one of the repository's `info/exclude`
  /// at a repository root, when the directory is resolved and again for each
  /// resolved ancestor directory on every `refresh_for_file` call. A
  /// repository root whose `.git` is a file (ex. a linked worktree) also has
  /// that file and its git directory's `commondir` read when it's resolved,
  /// and on every `refresh_for_file` call for as long as the repository has
  /// no `info/exclude`.
  pub detect_changes: bool,
}

/// Resolves gitignores in a directory tree taking into account
/// ancestor gitignores that may be found in a directory.
pub struct GitIgnoreTree<TEnvironment> {
  environment: TEnvironment,
  ignores: HashMap<PathBuf, CachedDir>,
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

  /// Drops what was resolved for the file's ancestor directories when a
  /// `.gitignore`, `info/exclude` or `.git` that it was resolved from has
  /// since been changed, added or removed, so the next resolution reads the
  /// files again. Does nothing unless the `detect_changes` option is set.
  ///
  /// This stats the files of each previously resolved ancestor directory up to
  /// the repository root and only reads a file again when its modified time or
  /// size differs, so it's cheap enough to do before every lookup.
  pub fn refresh_for_file(&mut self, file_path: &Path) {
    if !self.options.detect_changes {
      return;
    }
    let mut stale_dir = None;
    for dir_path in file_path.ancestors().skip(1) {
      // a directory that hasn't been resolved gets resolved from the file
      // system on lookup, but its ancestors might be resolved and out of date
      let Some(snapshot) = self.ignores.get(dir_path).and_then(|cached| cached.snapshot.as_ref()) else {
        continue;
      };
      let current_snapshot = self.take_dir_snapshot(dir_path, Some(snapshot));
      if *snapshot != current_snapshot {
        stale_dir = Some(dir_path);
      }
      if current_snapshot.repo.is_some() {
        break; // resolution doesn't look above the repository root
      }
    }
    if let Some(stale_dir) = stale_dir {
      // descendants hold onto what was resolved for their ancestors
      self.ignores.retain(|dir_path, _| !dir_path.starts_with(stale_dir));
    }
  }

  fn get_resolved_git_ignore_inner(&mut self, dir_path: &Path, hint: Option<DirEntriesHint>) -> Option<Rc<DirGitIgnores>> {
    let maybe_resolved = self.ignores.get(dir_path).map(|cached| cached.resolved.clone());
    if let Some(resolved) = maybe_resolved {
      resolved
    } else {
      let cached = self.resolve_gitignore_in_dir(dir_path, hint);
      let resolved = cached.resolved.clone();
      self.ignores.insert(dir_path.to_owned(), cached);
      resolved
    }
  }

  fn resolve_gitignore_in_dir(&mut self, dir_path: &Path, hint: Option<DirEntriesHint>) -> CachedDir {
    // take this before reading the files so a change made while resolving
    // is seen as a change the next time
    let snapshot = self.options.detect_changes.then(|| self.take_dir_snapshot(dir_path, None));
    // a directory containing `.git` is the root of a repository, which is
    // the only kind of directory an exclude file applies at
    let exclude_path = match (&snapshot, hint) {
      (Some(snapshot), _) => snapshot.repo.as_ref().map(|repo| repo.exclude_path.clone()),
      // skip the stat when the caller's listing already shows there's no `.git`
      (None, Some(hint)) if !hint.has_git => None,
      (None, _) => self.stat_git(dir_path).map(|git| self.resolve_info_exclude_path(dir_path, &git)),
    };
    let parent = if exclude_path.is_some() {
      None // don't search for gitignores above the repository root
    } else {
      // ancestors aren't part of the caller's listing, so resolve them without a hint
      dir_path.parent().and_then(|parent| self.get_resolved_git_ignore_inner(parent, None))
    };
    let current = self.resolve_current_gitignore(dir_path, exclude_path.as_deref(), hint);
    let resolved = if parent.is_none() && current.is_none() {
      None
    } else {
      Some(Rc::new(DirGitIgnores { current, parent }))
    };
    CachedDir { resolved, snapshot }
  }

  /// Resolves the gitignore of the directory itself, where the exclude path is
  /// the one of the repository's `info/exclude` when the directory is the root
  /// of a repository.
  fn resolve_current_gitignore(&self, dir_path: &Path, exclude_path: Option<&Path>, hint: Option<DirEntriesHint>) -> Option<Rc<ignore::gitignore::Gitignore>> {
    // skip the read when the caller's listing already shows there's no `.gitignore`
    let maybe_has_gitignore = hint.map(|h| h.has_gitignore).unwrap_or(true);
    let gitignore_text = if maybe_has_gitignore {
      self.environment.read_file(dir_path.join(".gitignore")).ok()
    } else {
      None
    };
    // git also reads the repository's `info/exclude`, treating it like an
    // uncommitted `.gitignore` at the repository root (https://git-scm.com/docs/gitignore).
    // Only the repo root can have this file, so avoid the read everywhere else.
    let is_repo_root = exclude_path.is_some();
    let exclude_text = exclude_path.and_then(|path| self.environment.read_file(path).ok());
    // git's global excludes file applies repository-wide, so resolve it at the
    // repo root where it becomes the parent of every descendant directory
    let global_lines: &[String] = if is_repo_root { self.options.global_gitignore_lines.as_slice() } else { &[] };
    if gitignore_text.is_none() && exclude_text.is_none() && global_lines.is_empty() {
      return None;
    }

    let mut builder = ignore::gitignore::GitignoreBuilder::new(dir_path);
    // git's precedence is global excludes < `info/exclude` < `.gitignore`,
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
    // override the gitignore contents to include these paths (escaping so a
    // path with glob characters in its name matches literally)
    for path in &self.options.include_paths {
      if let Ok(suffix) = path.strip_prefix(dir_path) {
        let mut line = format!("!/{}", escape_glob_text(&suffix.to_string_lossy().replace('\\', "/")));
        let _ignore = builder.add_line(None, &line);
        // also match it as a directory
        if !line["!/".len()..].ends_with('/') {
          line.push('/');
          let _ignore = builder.add_line(None, &line);
        }
      }
    }
    let gitignore = builder.build().ok()?;
    Some(Rc::new(gitignore))
  }

  /// Takes a snapshot of the directory, where the previous one is provided
  /// when there is one to save resolving what it already has.
  fn take_dir_snapshot(&self, dir_path: &Path, previous: Option<&DirSnapshot>) -> DirSnapshot {
    DirSnapshot {
      gitignore: self.take_file_snapshot(&dir_path.join(".gitignore")),
      repo: self.stat_git(dir_path).map(|git| {
        let exclude_path = match previous.and_then(|previous| previous.repo.as_ref()) {
          // the `.git` is the same as last time, so don't read the files that
          // lead to the path again. That's not done when no file was at the
          // path because then it might be wrong: the files could have been
          // read while git was creating them or the read could have failed.
          Some(previous) if previous.git == git && previous.exclude.is_some() => previous.exclude_path.clone(),
          _ => self.resolve_info_exclude_path(dir_path, &git),
        };
        RepoSnapshot {
          exclude: self.take_file_snapshot(&exclude_path),
          exclude_path,
          git,
        }
      }),
    }
  }

  /// Stats the `.git` in the directory, which is `None` when it doesn't have one.
  fn stat_git(&self, dir_path: &Path) -> Option<GitStat> {
    // not `path_exists` because that would debug log on every refresh
    let metadata = self.environment.fs_metadata(dir_path.join(".git")).ok()?;
    Some(if metadata.file_type().is_file() {
      GitStat::File(FileSnapshot::from_metadata(&metadata))
    } else {
      GitStat::Dir
    })
  }

  /// Resolves the path of the `info/exclude` of the repository the directory
  /// is the root of.
  ///
  /// It's in `.git` except when that's a file with the path of the repository's
  /// git directory, which is the case in a linked worktree (`git worktree add`)
  /// and in a submodule (https://git-scm.com/docs/gitrepository-layout).
  fn resolve_info_exclude_path(&self, dir_path: &Path, git: &GitStat) -> PathBuf {
    let git_path = dir_path.join(".git");
    let git_dir = match git {
      // git doesn't read a file that's larger than this either
      GitStat::File(file) if file.len <= MAX_GIT_FILE_LEN => self.resolve_git_file_common_dir(&git_path),
      GitStat::File(_) | GitStat::Dir => None,
    };
    git_dir.unwrap_or(git_path).join("info").join("exclude")
  }

  /// Resolves the git directory a `.git` file leads to that's shared by all the
  /// worktrees of the repository, which is where git reads `info/exclude` from.
  fn resolve_git_file_common_dir(&self, git_file_path: &Path) -> Option<PathBuf> {
    let text = self.environment.read_file(git_file_path).ok()?;
    let git_dir = text.strip_prefix("gitdir: ")?.trim_end_matches(['\r', '\n']);
    if git_dir.is_empty() {
      return None;
    }
    // a relative path is relative to the directory the `.git` file is in
    let git_dir = git_file_path.parent()?.join(git_dir);
    // the git directory of a linked worktree only has the files that are
    // specific to the worktree and its `commondir` file has the path of the
    // git directory with the rest (relative to the worktree's git directory)
    Some(match self.environment.read_file(git_dir.join("commondir")) {
      Ok(text) => git_dir.join(text.trim_end_matches(['\r', '\n'])),
      Err(_) => git_dir, // ex. a submodule
    })
  }

  fn take_file_snapshot(&self, file_path: &Path) -> Option<FileSnapshot> {
    let metadata = self.environment.fs_metadata(file_path).ok()?;
    Some(FileSnapshot::from_metadata(&metadata))
  }
}

/// What was resolved for a directory.
struct CachedDir {
  resolved: Option<Rc<DirGitIgnores>>,
  /// Only set when detecting changes.
  snapshot: Option<DirSnapshot>,
}

/// State of the files a directory's gitignore gets resolved from, which
/// is compared to detect when the directory needs to be resolved again.
#[derive(PartialEq, Eq)]
struct DirSnapshot {
  /// `None` when there's no `.gitignore` in the directory.
  gitignore: Option<FileSnapshot>,
  /// `None` when there's no `.git` in the directory, which means it's not
  /// the root of a repository.
  repo: Option<RepoSnapshot>,
}

/// State of what's specific to a directory that's the root of a repository.
#[derive(PartialEq, Eq)]
struct RepoSnapshot {
  git: GitStat,
  /// Path of the repository's `info/exclude`, which is kept so the file can
  /// be checked for changes without resolving where it is again.
  exclude_path: PathBuf,
  /// `None` when the repository has no `info/exclude`.
  exclude: Option<FileSnapshot>,
}

/// State of the `.git` in the root directory of a repository.
#[derive(PartialEq, Eq)]
enum GitStat {
  /// The repository's git directory, which has no state to compare because
  /// most git commands modify it.
  Dir,
  /// A file with the path of the repository's git directory.
  File(FileSnapshot),
}

#[derive(PartialEq, Eq)]
struct FileSnapshot {
  modified: Option<SystemTime>,
  len: u64,
}

impl FileSnapshot {
  fn from_metadata(metadata: &impl FsMetadataValue) -> Self {
    Self {
      modified: metadata.modified().ok(),
      len: metadata.len(),
    }
  }
}

/// Size above which git refuses to read a `.git` file.
const MAX_GIT_FILE_LEN: u64 = 1 << 20;

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
  fn refresh_detects_changed_added_and_removed_gitignores() {
    let env = TestEnvironment::new();
    env.set_fs_time(1000);
    env.mk_dir_all("/sub_dir/nested").unwrap();
    env.mk_dir_all("/other_dir").unwrap();
    env.write_file("/.gitignore", "a.txt").unwrap();
    let mut ignore_tree = new_change_detecting_tree(&env);
    let mut assert_ignored = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      ignore_tree.refresh_for_file(&path);
      let is_ignored = ignore_tree
        .get_resolved_git_ignore_for_file(&path)
        .map(|gitignore| gitignore.is_ignored(&path, /* is_dir */ false))
        .unwrap_or(false);
      assert_eq!(is_ignored, expected, "Path: {}", path.display());
    };
    assert_ignored("/a.txt", true);
    assert_ignored("/b.txt", false);
    assert_ignored("/sub_dir/nested/a.txt", true);
    assert_ignored("/other_dir/b.txt", false);

    // changed to text of the same length, so only the modified time differs
    env.set_fs_time(2000);
    env.write_file("/.gitignore", "b.txt").unwrap();
    assert_ignored("/a.txt", false);
    assert_ignored("/b.txt", true);
    // directories that weren't asked about since the change
    assert_ignored("/sub_dir/nested/a.txt", false);
    assert_ignored("/other_dir/b.txt", true);

    // changed without the modified time changing, so only the length differs
    env.write_file("/.gitignore", "b.txt\nc.txt").unwrap();
    assert_ignored("/sub_dir/c.txt", true);

    // added in a directory that was resolved as not having one
    env.write_file("/sub_dir/.gitignore", "!b.txt").unwrap();
    assert_ignored("/sub_dir/nested/b.txt", false);
    assert_ignored("/other_dir/b.txt", true);

    // removed
    env.remove_file("/sub_dir/.gitignore").unwrap();
    assert_ignored("/sub_dir/nested/b.txt", true);
    env.remove_file("/.gitignore").unwrap();
    assert_ignored("/sub_dir/nested/b.txt", false);
    assert_ignored("/b.txt", false);
  }

  #[test]
  fn refresh_detects_git_dir_and_info_exclude_changes() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/repo/sub_dir").unwrap();
    env.write_file("/.gitignore", "a.txt").unwrap();
    let mut ignore_tree = new_change_detecting_tree(&env);
    let mut assert_ignored = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      ignore_tree.refresh_for_file(&path);
      let is_ignored = ignore_tree
        .get_resolved_git_ignore_for_file(&path)
        .map(|gitignore| gitignore.is_ignored(&path, /* is_dir */ false))
        .unwrap_or(false);
      assert_eq!(is_ignored, expected, "Path: {}", path.display());
    };
    assert_ignored("/repo/sub_dir/a.txt", true);

    // the directory becomes a repository root, so gitignores above it no longer apply
    env.mk_dir_all("/repo/.git/info").unwrap();
    assert_ignored("/repo/sub_dir/a.txt", false);
    assert_ignored("/repo/sub_dir/b.txt", false);

    env.write_file("/repo/.git/info/exclude", "b.txt").unwrap();
    assert_ignored("/repo/sub_dir/b.txt", true);
    env.write_file("/repo/.git/info/exclude", "other.txt").unwrap();
    assert_ignored("/repo/sub_dir/b.txt", false);

    // and back to not being a repository root
    env.remove_dir_all("/repo/.git").unwrap();
    assert_ignored("/repo/sub_dir/a.txt", true);
  }

  #[test]
  fn refresh_keeps_what_was_resolved_when_nothing_changed() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/sub_dir").unwrap();
    env.write_file("/.gitignore", "a.txt").unwrap();
    let path = PathBuf::from("/sub_dir/a.txt");

    let mut ignore_tree = new_change_detecting_tree(&env);
    let first = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
    ignore_tree.refresh_for_file(&path);
    let second = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
    assert!(Rc::ptr_eq(&first, &second));

    // not detecting changes is the default, where a refresh does nothing
    let mut ignore_tree = GitIgnoreTree::new(env.clone(), GitIgnoreTreeOptions::default());
    let first = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
    env.write_file("/.gitignore", "other.txt").unwrap();
    ignore_tree.refresh_for_file(&path);
    let second = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
    assert!(Rc::ptr_eq(&first, &second));
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

  #[test]
  fn honours_git_info_exclude_in_linked_worktree() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/main/.git/info").unwrap();
    env.write_file("/main/.git/info/exclude", "from_exclude.txt").unwrap();
    env.write_file("/main/.gitignore", "from_main_gitignore.txt").unwrap();
    // what `git worktree add` creates
    env.mk_dir_all("/main/.git/worktrees/absolute").unwrap();
    env.write_file("/main/.git/worktrees/absolute/commondir", "../..\n").unwrap();
    env.mk_dir_all("/absolute/sub_dir").unwrap();
    env.write_file("/absolute/.git", "gitdir: /main/.git/worktrees/absolute\n").unwrap();
    env.write_file("/absolute/.gitignore", "from_gitignore.txt").unwrap();
    // and with `worktree.useRelativePaths` and Windows line endings
    env.mk_dir_all("/main/.git/worktrees/relative").unwrap();
    env.write_file("/main/.git/worktrees/relative/commondir", "../..\r\n").unwrap();
    env.mk_dir_all("/main/relative").unwrap();
    env.write_file("/main/relative/.git", "gitdir: ../.git/worktrees/relative\r\n").unwrap();
    // git also accepts an absolute path in a `commondir`
    env.mk_dir_all("/main/.git/worktrees/absolute_commondir").unwrap();
    env.write_file("/main/.git/worktrees/absolute_commondir/commondir", "/main/.git\n").unwrap();
    env.mk_dir_all("/absolute_commondir").unwrap();
    env
      .write_file("/absolute_commondir/.git", "gitdir: /main/.git/worktrees/absolute_commondir\n")
      .unwrap();
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    let mut run_test = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
      assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
    };
    run_test("/absolute/from_exclude.txt", true);
    run_test("/absolute/sub_dir/from_exclude.txt", true);
    run_test("/absolute/from_gitignore.txt", true);
    run_test("/absolute/other.txt", false);
    // only the exclude file is shared with the main worktree
    run_test("/absolute/from_main_gitignore.txt", false);
    run_test("/main/relative/from_exclude.txt", true);
    // the worktree is a repository root, so gitignores above it don't apply
    run_test("/main/relative/from_main_gitignore.txt", false);
    run_test("/absolute_commondir/from_exclude.txt", true);
    run_test("/main/from_exclude.txt", true);
  }

  #[test]
  fn honours_git_info_exclude_in_submodule() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/.git/info").unwrap();
    env.write_file("/.git/info/exclude", "from_super_exclude.txt").unwrap();
    // a submodule's git directory isn't shared, so it has no `commondir`
    env.mk_dir_all("/.git/modules/sub/info").unwrap();
    env.write_file("/.git/modules/sub/info/exclude", "from_exclude.txt").unwrap();
    env.mk_dir_all("/sub").unwrap();
    env.write_file("/sub/.git", "gitdir: ../.git/modules/sub\n").unwrap();
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    let mut run_test = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
      assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
    };
    run_test("/sub/from_exclude.txt", true);
    run_test("/sub/from_super_exclude.txt", false);
    run_test("/from_super_exclude.txt", true);
    run_test("/from_exclude.txt", false);
  }

  #[test]
  fn git_file_that_does_not_lead_to_an_info_exclude() {
    let env = TestEnvironment::new();
    env.write_file("/.gitignore", "above.txt").unwrap();
    // a git directory that would be used if these were followed
    env.mk_dir_all("/git_dir/info").unwrap();
    env.write_file("/git_dir/info/exclude", "from_exclude.txt").unwrap();
    // git doesn't read a `.git` file that's over a megabyte
    let too_large = format!("gitdir: /git_dir{}", "\n".repeat(1 << 20));
    let dirs = [
      ("/not_a_git_file", "/git_dir"),
      ("/wrong_prefix", "gitdir:/git_dir\n"),
      ("/no_path", "gitdir: \n"),
      ("/missing_git_dir", "gitdir: /missing\n"),
      ("/too_large", too_large.as_str()),
    ];
    for (dir, git_file_text) in dirs {
      env.mk_dir_all(dir).unwrap();
      env.write_file(format!("{dir}/.git"), git_file_text).unwrap();
      env.write_file(format!("{dir}/.gitignore"), "from_gitignore.txt").unwrap();
      // not used because the directory isn't a git directory
      env.mk_dir_all(format!("{dir}/info")).unwrap();
      env.write_file(format!("{dir}/info/exclude"), "from_exclude.txt").unwrap();
    }
    let mut ignore_tree = GitIgnoreTree::new(env, GitIgnoreTreeOptions::default());
    for (dir, _) in dirs {
      let mut run_test = |name: &str, expected: bool| {
        let path = PathBuf::from(format!("{dir}/{name}"));
        let gitignore = ignore_tree.get_resolved_git_ignore_for_file(&path).unwrap();
        assert_eq!(gitignore.is_ignored(&path, /* is_dir */ false), expected, "Path: {}", path.display());
      };
      run_test("from_gitignore.txt", true);
      run_test("from_exclude.txt", false);
      // still a repository root
      run_test("above.txt", false);
    }
  }

  #[test]
  fn refresh_detects_info_exclude_changes_in_linked_worktree() {
    let env = TestEnvironment::new();
    env.set_fs_time(1000);
    for main in ["/main", "/other_main"] {
      env.mk_dir_all(format!("{main}/.git/info")).unwrap();
      env.mk_dir_all(format!("{main}/.git/worktrees/linked")).unwrap();
      env.write_file(format!("{main}/.git/worktrees/linked/commondir"), "../..\n").unwrap();
    }
    env.mk_dir_all("/linked/sub_dir").unwrap();
    env.write_file("/linked/.git", "gitdir: /main/.git/worktrees/linked\n").unwrap();
    let mut ignore_tree = new_change_detecting_tree(&env);
    let mut assert_ignored = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      ignore_tree.refresh_for_file(&path);
      let is_ignored = ignore_tree
        .get_resolved_git_ignore_for_file(&path)
        .map(|gitignore| gitignore.is_ignored(&path, /* is_dir */ false))
        .unwrap_or(false);
      assert_eq!(is_ignored, expected, "Path: {}", path.display());
    };
    assert_ignored("/linked/sub_dir/a.txt", false);

    // added, changed and removed in the main worktree's git directory
    env.write_file("/main/.git/info/exclude", "a.txt").unwrap();
    assert_ignored("/linked/sub_dir/a.txt", true);
    assert_ignored("/linked/sub_dir/b.txt", false);
    env.set_fs_time(2000);
    env.write_file("/main/.git/info/exclude", "b.txt").unwrap();
    assert_ignored("/linked/sub_dir/a.txt", false);
    assert_ignored("/linked/sub_dir/b.txt", true);
    env.remove_file("/main/.git/info/exclude").unwrap();
    assert_ignored("/linked/sub_dir/b.txt", false);

    // the files that lead to the exclude file aren't read when nothing changed
    env.write_file("/main/.git/info/exclude", "a.txt").unwrap();
    assert_ignored("/linked/sub_dir/a.txt", true);
    let read_counts = |env: &TestEnvironment| {
      (
        env.file_read_count("/linked/.git"),
        env.file_read_count("/main/.git/worktrees/linked/commondir"),
      )
    };
    let counts = read_counts(&env);
    assert_ignored("/linked/sub_dir/a.txt", true);
    assert_ignored("/linked/a.txt", true);
    assert_ignored("/linked/sub_dir/b.txt", false);
    assert_eq!(read_counts(&env), counts);

    // the `.git` file is changed to make it a worktree of another repository,
    // whose exclude file has the same modified time and size
    env.write_file("/other_main/.git/info/exclude", "b.txt").unwrap();
    env.write_file("/linked/.git", "gitdir: /other_main/.git/worktrees/linked\n").unwrap();
    assert_ignored("/linked/sub_dir/a.txt", false);
    assert_ignored("/linked/sub_dir/b.txt", true);

    // and to having its own git directory
    env.remove_file("/linked/.git").unwrap();
    env.mk_dir_all("/linked/.git/info").unwrap();
    assert_ignored("/linked/sub_dir/b.txt", false);
    env.write_file("/linked/.git/info/exclude", "c.txt").unwrap();
    assert_ignored("/linked/sub_dir/c.txt", true);
  }

  #[test]
  fn refresh_detects_git_file_starting_to_lead_to_an_info_exclude() {
    let env = TestEnvironment::new();
    env.set_fs_time(1000);
    env.mk_dir_all("/main/.git/info").unwrap();
    env.write_file("/main/.git/info/exclude", "a.txt").unwrap();
    env.mk_dir_all("/main/.git/worktrees/linked").unwrap();
    env.mk_dir_all("/linked").unwrap();
    // the same length as what it gets changed to
    env.write_file("/linked/.git", "gitdir: /nain/.git/worktrees/linked\n").unwrap();
    let mut ignore_tree = new_change_detecting_tree(&env);
    let mut assert_ignored = |path: &str, expected: bool| {
      let path = PathBuf::from(path);
      ignore_tree.refresh_for_file(&path);
      let is_ignored = ignore_tree
        .get_resolved_git_ignore_for_file(&path)
        .map(|gitignore| gitignore.is_ignored(&path, /* is_dir */ false))
        .unwrap_or(false);
      assert_eq!(is_ignored, expected, "Path: {}", path.display());
    };
    assert_ignored("/linked/a.txt", false);

    // the git directory exists, but resolved while git was still creating the
    // worktree, which is before it has a `commondir`
    env.set_fs_time(2000);
    env.write_file("/linked/.git", "gitdir: /main/.git/worktrees/linked\n").unwrap();
    assert_ignored("/linked/a.txt", false);

    // the `.git` file hasn't changed since it was last looked at
    env.write_file("/main/.git/worktrees/linked/commondir", "../..\n").unwrap();
    assert_ignored("/linked/a.txt", true);
  }

  fn new_change_detecting_tree(env: &TestEnvironment) -> GitIgnoreTree<TestEnvironment> {
    GitIgnoreTree::new(
      env.clone(),
      GitIgnoreTreeOptions {
        detect_changes: true,
        ..Default::default()
      },
    )
  }
}
