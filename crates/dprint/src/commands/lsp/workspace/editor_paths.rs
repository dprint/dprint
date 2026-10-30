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
  /// Remembers the editor's path of the canonical path, which is used for the
  /// directories they have in common (ex. `/link/project/file.ts` and
  /// `/real/project/file.ts` are `/link` and `/real`).
  pub fn add(&self, editor_path: &Path, canonical_path: &Path) {
    if editor_path == canonical_path {
      return;
    }
    let mut editor_dir = editor_path;
    let mut canonical_dir = canonical_path;
    // the components they end with are the same
    while let (Some(editor_name), Some(canonical_name)) = (editor_dir.file_name(), canonical_dir.file_name()) {
      if editor_name != canonical_name {
        break;
      }
      match (editor_dir.parent(), canonical_dir.parent()) {
        (Some(editor_parent), Some(canonical_parent)) => {
          editor_dir = editor_parent;
          canonical_dir = canonical_parent;
        }
        _ => break,
      }
    }
    if editor_dir == canonical_dir {
      return;
    }
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
    paths.add(Path::new("/link/project/file.ts"), Path::new("/real/project/file.ts"));
    assert_eq!(paths.get(Path::new("/real/project/dprint.json")), PathBuf::from("/link/project/dprint.json"));
    assert_eq!(paths.get(Path::new("/real")), PathBuf::from("/link"));
    // a directory the editor doesn't know by another path
    assert_eq!(paths.get(Path::new("/other/dprint.json")), PathBuf::from("/other/dprint.json"));
    // the same paths don't change anything
    paths.add(Path::new("/other/file.ts"), Path::new("/other/file.ts"));
    assert_eq!(paths.get(Path::new("/other/file.ts")), PathBuf::from("/other/file.ts"));
    // the closest directory wins
    paths.add(Path::new("/link/project/nested-link/file.ts"), Path::new("/elsewhere/file.ts"));
    assert_eq!(paths.get(Path::new("/elsewhere/a.ts")), PathBuf::from("/link/project/nested-link/a.ts"));
    assert_eq!(paths.get(Path::new("/real/project/a.ts")), PathBuf::from("/link/project/a.ts"));
  }
}
