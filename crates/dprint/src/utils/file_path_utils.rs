use std::path::Path;

pub fn get_lowercase_file_extension(file_path: &Path) -> Option<String> {
  file_path
    .extension()
    .and_then(|e| e.to_str())
    .map(|ext| String::from(ext).to_lowercase())
    .or_else(|| {
      if file_path.components().count() == 1 {
        let text = file_path.to_string_lossy();
        if let Some(index) = text.rfind('.')
          && index == 0
        {
          return Some(text[1..].to_lowercase());
        }
      }
      None
    })
}

pub fn get_lowercase_file_name(file_path: &Path) -> Option<String> {
  file_path.file_name().and_then(|s| s.to_str()).map(|s| s.to_lowercase())
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
}
