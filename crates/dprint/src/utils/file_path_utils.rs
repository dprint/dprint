use std::path::Path;

pub fn get_lowercase_file_extension(file_path: &Path) -> Option<String> {
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        Some(String::from(ext).to_lowercase())
    } else {
        None
    }
}

pub fn get_lowercase_exact_file_names(file_path: &Path) -> Option<String> {
    file_path.file_name().and_then(|s| s.to_str()).map(|s| s.to_lowercase())
}
