use std::path::PathBuf;

pub fn get_lowercase_file_extension(file_path: &PathBuf) -> Option<String> {
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        Some(String::from(ext).to_lowercase())
    } else {
        None
    }
}
