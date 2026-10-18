use std::borrow::Cow;
use std::path::Path;

pub fn get_lowercase_file_extension(file_path: &Path) -> Option<Cow<'_, str>> {
  if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
    return Some(to_lowercase_cow(ext));
  }
  // a path that's only a file name like `.txt` has no extension as far as
  // `Path` is concerned, but dprint treats the text after the dot as one
  if file_path.components().count() == 1 {
    let text = file_path.to_string_lossy();
    if text.rfind('.') == Some(0) {
      return Some(Cow::Owned(text[1..].to_lowercase()));
    }
  }
  None
}

pub fn get_lowercase_file_name(file_path: &Path) -> Option<Cow<'_, str>> {
  file_path.file_name().and_then(|s| s.to_str()).map(to_lowercase_cow)
}

/// Lowercases the text without allocating when it's already lowercase ascii,
/// which is the common case for a file name or extension.
fn to_lowercase_cow(text: &str) -> Cow<'_, str> {
  if text.is_ascii() && !text.bytes().any(|b| b.is_ascii_uppercase()) {
    Cow::Borrowed(text)
  } else {
    Cow::Owned(text.to_lowercase())
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_get_lowercase_file_extension() {
    assert_eq!(get_lowercase_file_extension(Path::new("test.txt")).unwrap(), "txt");
    assert_eq!(get_lowercase_file_extension(Path::new("test.txT")).unwrap(), "txt");
    assert_eq!(get_lowercase_file_extension(Path::new(".txt")).unwrap(), "txt");
    assert_eq!(get_lowercase_file_extension(Path::new(".Txt")).unwrap(), "txt");
    assert!(get_lowercase_file_extension(Path::new("txt")).is_none());
    assert!(get_lowercase_file_extension(Path::new("/path/.txt")).is_none());
    assert_eq!(get_lowercase_file_extension(Path::new("/path/test.txt")).unwrap(), "txt");
  }

  #[test]
  fn test_get_lowercase_file_name() {
    assert_eq!(get_lowercase_file_name(Path::new("/path/Test.TXT")).unwrap(), "test.txt");
    assert_eq!(get_lowercase_file_name(Path::new("/path/test.txt")).unwrap(), "test.txt");
    assert!(get_lowercase_file_name(Path::new("/")).is_none());
  }

  #[test]
  fn test_to_lowercase_cow() {
    // already lowercase ascii, so it shouldn't allocate
    assert!(matches!(to_lowercase_cow("test.txt"), Cow::Borrowed(_)));
    assert!(matches!(to_lowercase_cow(""), Cow::Borrowed(_)));
    assert!(matches!(to_lowercase_cow("Test.txt"), Cow::Owned(_)));
    // non-ascii always goes through `to_lowercase` so nothing it handles is missed
    assert_eq!(to_lowercase_cow("\u{00c9}t\u{00c9}"), "\u{00e9}t\u{00e9}");
    assert!(matches!(to_lowercase_cow("\u{00e9}"), Cow::Owned(_)));
  }
}
