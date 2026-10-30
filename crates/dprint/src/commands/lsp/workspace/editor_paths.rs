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
  /// of it (ex. a symlink to it), and replaces the one remembered for it.
  pub fn add(&self, editor_dir: &Path, canonical_dir: &Path) {
    let mut dirs = self.dirs.borrow_mut();
    dirs.retain(|(dir, _)| dir != canonical_dir);
    dirs.push((canonical_dir.to_path_buf(), editor_dir.to_path_buf()));
  }

  /// Gets the editor's path of the canonical path, which is the canonical path
  /// when the editor doesn't know it by another one.
  pub fn get(&self, canonical_path: &Path) -> PathBuf {
    let dirs = self.dirs.borrow();
    // the closest directory wins (ex. a symlink within a symlinked directory)
    let closest = dirs
      .iter()
      .filter(|(dir, _)| canonical_path.starts_with(dir))
      .max_by_key(|(dir, _)| dir.components().count());
    match closest {
      Some((dir, editor_dir)) => match canonical_path.strip_prefix(dir) {
        Ok(relative) if relative.as_os_str().is_empty() => editor_dir.clone(),
        Ok(relative) => editor_dir.join(relative),
        Err(_) => canonical_path.to_path_buf(),
      },
      None => canonical_path.to_path_buf(),
    }
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
  }
}
