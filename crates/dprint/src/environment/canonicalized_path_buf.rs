use std::borrow::Cow;
use std::path::Display;
use std::path::Path;
use std::path::PathBuf;
use std::path::StripPrefixError;

/// A PathBuf that is guaranteed to be canonicalized.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonicalizedPathBuf {
  path: PathBuf,
}

impl CanonicalizedPathBuf {
  pub(super) fn new(path: PathBuf) -> Self {
    CanonicalizedPathBuf { path }
  }

  #[cfg(test)]
  pub fn new_for_testing(path: impl AsRef<Path>) -> CanonicalizedPathBuf {
    assert!(path.as_ref().starts_with("/") || path.as_ref().starts_with("C:\\") || path.as_ref().starts_with("V:\\") || path.as_ref().starts_with("\\?\\UNC"));
    CanonicalizedPathBuf::new(path.as_ref().to_path_buf())
  }

  pub fn into_path_buf(self) -> PathBuf {
    self.path
  }

  pub fn display(&self) -> Display<'_> {
    self.path.display()
  }

  pub fn starts_with(&self, other: &CanonicalizedPathBuf) -> bool {
    self.path.starts_with(&other.path)
  }

  pub fn to_string_lossy(&self) -> Cow<'_, str> {
    self.path.to_string_lossy()
  }

  pub fn strip_prefix(&self, base: &CanonicalizedPathBuf) -> Result<&Path, StripPrefixError> {
    self.path.strip_prefix(&base.path)
  }

  pub fn join(&self, path: impl AsRef<Path>) -> PathBuf {
    self.path.join(path)
  }

  pub fn parent(&self) -> Option<CanonicalizedPathBuf> {
    self.path.parent().map(|p| CanonicalizedPathBuf::new(p.to_path_buf()))
  }

  pub fn join_panic_relative(&self, path: impl AsRef<str>) -> CanonicalizedPathBuf {
    // this is to prevent an unresolved path from being created
    if path.as_ref().contains("../") {
      panic!("Cannot join to {} because the path had ../ ({}).", self.path.display(), path.as_ref());
    }

    CanonicalizedPathBuf::new(self.path.join(path.as_ref()))
  }
}

impl AsRef<Path> for CanonicalizedPathBuf {
  fn as_ref(&self) -> &Path {
    self.path.as_path()
  }
}
