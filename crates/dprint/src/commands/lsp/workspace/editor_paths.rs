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
}

impl EditorPaths {
  /// Remembers the editor's path of a canonical directory, which is an alias
  /// of it (ex. a symlink to it), and replaces the one remembered for it and
  /// the directory the editor's path was an alias of before.
  pub fn add(&self, editor_dir: &Path, canonical_dir: &Path) {
    let mut dirs = self.dirs.borrow_mut();
    dirs.retain(|(dir, dir_editor_path)| dir != canonical_dir && dir_editor_path != editor_dir);
    dirs.push((canonical_dir.to_path_buf(), editor_dir.to_path_buf()));
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
    // the most recent path replaces the one before it
    paths.add(Path::new("/other-link"), Path::new("/real/project"));
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/other-link/a.ts"));
    // and an editor's path that's an alias of another directory now isn't one of the one before
    paths.add(Path::new("/other-link"), Path::new("/real/moved"));
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/real/project/a.ts"));
    assert_eq!(paths.get(Path::new("/real/moved/a.ts")), PathBuf::from("/other-link/a.ts"));
    paths.remove(Path::new("/real/moved"));
    assert_eq!(paths.get(Path::new("/real/moved/a.ts")), PathBuf::from("/real/moved/a.ts"));
  }
}
