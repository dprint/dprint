use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

/// The directories the editor knows by another path than their canonical one
/// (ex. a workspace folder opened through a symlink), so the paths the server
/// provides are the editor's while it uses the canonical ones itself.
#[derive(Debug, Default)]
pub struct EditorPaths {
  /// The canonical path of each directory with the editor's path of it, which
  /// is the most recent one the editor used.
  dirs: RefCell<Vec<(PathBuf, PathBuf)>>,
  /// The canonical path of each directory with an editor's path of it that
  /// another one replaced (ex. another symlink to it), which led to it then.
  replaced_dirs: RefCell<Vec<(PathBuf, PathBuf)>>,
}

impl EditorPaths {
  /// Remembers the editor's path of a canonical directory, which is an alias
  /// of it (ex. a symlink to it), and replaces the one remembered for it and
  /// the directory the editor's path was an alias of before, which are
  /// provided (see `ReplacedEditorPaths`).
  pub fn add(&self, editor_dir: &Path, canonical_dir: &Path) -> ReplacedEditorPaths {
    let mut dirs = self.dirs.borrow_mut();
    let mut replaced = ReplacedEditorPaths::default();
    dirs.retain(|(dir, dir_editor_path)| {
      if dir_editor_path == editor_dir && dir != canonical_dir {
        replaced.other_canonical_dir = Some(dir.clone());
      } else if dir == canonical_dir && dir_editor_path != editor_dir {
        replaced.other_editor_dir = Some(dir_editor_path.clone());
      }
      dir != canonical_dir && dir_editor_path != editor_dir
    });
    dirs.push((canonical_dir.to_path_buf(), editor_dir.to_path_buf()));
    let mut replaced_dirs = self.replaced_dirs.borrow_mut();
    replaced_dirs.retain(|(_, dir_editor_path)| dir_editor_path != editor_dir && Some(dir_editor_path) != replaced.other_editor_dir.as_ref());
    if let Some(other_editor_dir) = &replaced.other_editor_dir {
      replaced_dirs.push((canonical_dir.to_path_buf(), other_editor_dir.clone()));
    }
    replaced
  }

  /// Gets the editor's paths of directories that another one replaced, which
  /// are canonical directories with an editor's path of each (see `add`).
  pub fn replaced_dirs(&self) -> Vec<(PathBuf, PathBuf)> {
    self.replaced_dirs.borrow().clone()
  }

  /// Forgets an editor's path of a directory that another one replaced (ex.
  /// because it doesn't lead to it anymore).
  pub fn remove_replaced(&self, editor_dir: &Path) {
    self.replaced_dirs.borrow_mut().retain(|(_, dir_editor_path)| dir_editor_path != editor_dir);
  }

  /// Gets the alias whose editor's path the path is in, which is a canonical
  /// directory with the editor's path of it.
  pub fn get_alias_of_editor_path(&self, editor_path: &Path) -> Option<(PathBuf, PathBuf)> {
    self
      .dirs
      .borrow()
      .iter()
      .filter(|(_, editor_dir)| editor_path.starts_with(editor_dir))
      .max_by_key(|(_, editor_dir)| editor_dir.components().count())
      .cloned()
  }

  /// Gets the editor's path of the canonical path, which is the canonical path
  /// when the editor doesn't know it by another one.
  pub fn get(&self, canonical_path: &Path) -> PathBuf {
    match self.get_alias(canonical_path) {
      Some((dir, editor_dir)) => match canonical_path.strip_prefix(&dir) {
        Ok(relative) if relative.as_os_str().is_empty() => editor_dir,
        Ok(relative) => editor_dir.join(relative),
        Err(_) => canonical_path.to_path_buf(),
      },
      None => canonical_path.to_path_buf(),
    }
  }

  /// Gets the alias that the editor's path of the canonical path is through,
  /// which is a canonical directory with the editor's path of it.
  pub fn get_alias(&self, canonical_path: &Path) -> Option<(PathBuf, PathBuf)> {
    // the closest directory wins (ex. a symlink within a symlinked directory)
    self
      .dirs
      .borrow()
      .iter()
      .filter(|(dir, _)| canonical_path.starts_with(dir))
      .max_by_key(|(dir, _)| dir.components().count())
      .cloned()
  }

  /// Forgets the editor's path of the canonical directory (ex. because it
  /// isn't an alias of it anymore).
  pub fn remove(&self, canonical_dir: &Path) {
    self.dirs.borrow_mut().retain(|(dir, _)| dir != canonical_dir);
  }
}

/// What `EditorPaths::add` replaced.
#[derive(Debug, Default, PartialEq)]
pub struct ReplacedEditorPaths {
  /// The canonical directory the editor's path was an alias of before, whose
  /// editor's paths name other files now.
  pub other_canonical_dir: Option<PathBuf>,
  /// The editor's path the canonical directory had before (ex. another
  /// symlink to it), which still leads to the same files.
  pub other_editor_dir: Option<PathBuf>,
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_get_the_editors_path_of_a_canonical_path() {
    let paths = EditorPaths::default();
    assert_eq!(paths.get(Path::new("/real/project/file.ts")), PathBuf::from("/real/project/file.ts"));
    paths.add(Path::new("/link/project"), Path::new("/real/project"));
    assert_eq!(paths.get(Path::new("/real/project/dprint.json")), PathBuf::from("/link/project/dprint.json"));
    assert_eq!(paths.get(Path::new("/real/project")), PathBuf::from("/link/project"));
    // a directory the editor doesn't know by another path
    assert_eq!(paths.get(Path::new("/real/other/dprint.json")), PathBuf::from("/real/other/dprint.json"));
    // the closest directory wins
    paths.add(Path::new("/link/project/nested-link"), Path::new("/elsewhere"));
    assert_eq!(paths.get(Path::new("/elsewhere/a.ts")), PathBuf::from("/link/project/nested-link/a.ts"));
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/link/project/a.ts"));
    // the most recent path replaces the one before it, which still leads to it
    assert_eq!(
      paths.add(Path::new("/other-link"), Path::new("/real/project")),
      ReplacedEditorPaths {
        other_canonical_dir: None,
        other_editor_dir: Some(PathBuf::from("/link/project")),
      }
    );
    assert_eq!(paths.replaced_dirs(), vec![(PathBuf::from("/real/project"), PathBuf::from("/link/project"))]);
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/other-link/a.ts"));
    // and an editor's path that's an alias of another directory now isn't one of the one before, which is provided
    assert_eq!(
      paths.add(Path::new("/other-link"), Path::new("/real/moved")),
      ReplacedEditorPaths {
        other_canonical_dir: Some(PathBuf::from("/real/project")),
        other_editor_dir: None,
      }
    );
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/real/project/a.ts"));
    assert_eq!(paths.get(Path::new("/real/moved/a.ts")), PathBuf::from("/other-link/a.ts"));
    paths.remove(Path::new("/real/moved"));
    // and both at once, when the editor's path led to another directory and the directory had another one
    paths.add(Path::new("/a"), Path::new("/x"));
    paths.add(Path::new("/b"), Path::new("/y"));
    assert_eq!(
      paths.add(Path::new("/a"), Path::new("/y")),
      ReplacedEditorPaths {
        other_canonical_dir: Some(PathBuf::from("/x")),
        other_editor_dir: Some(PathBuf::from("/b")),
      }
    );
    paths.remove_replaced(Path::new("/b"));
    assert!(!paths.replaced_dirs().iter().any(|(_, editor_dir)| editor_dir == Path::new("/b")));
    assert_eq!(paths.get(Path::new("/real/moved/a.ts")), PathBuf::from("/real/moved/a.ts"));
  }
}
