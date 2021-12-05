use std::path::Path;

pub fn get_lowercase_file_extension(file_path: &Path) -> Option<String> {
  file_path.extension().and_then(|e| e.to_str()).map(|ext| String::from(ext).to_lowercase())
}

pub fn get_lowercase_file_name(file_path: &Path) -> Option<String> {
  file_path.file_name().and_then(|s| s.to_str()).map(|s| s.to_lowercase())
}
