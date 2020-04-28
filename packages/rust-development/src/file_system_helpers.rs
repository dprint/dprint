use std::fs::{self};
use std::path::PathBuf;
use super::*;

pub fn get_specs_in_dir(path: &PathBuf, parse_spec_options: &ParseSpecOptions) -> Vec<(String, Spec)> {
    let mut result: Vec<(String, Spec)> = Vec::new();
    let spec_files = get_files_in_dir_recursive(&path);
    for (file_path, text) in spec_files {
        let specs = parse_specs(text, parse_spec_options);
        let lower_case_file_path = file_path.to_ascii_lowercase();
        let path_has_only = lower_case_file_path.contains("_only.txt") || lower_case_file_path.contains("_only/") || lower_case_file_path.contains("_only\\");
        let is_only_file = path_has_only && !specs.iter().any(|spec| spec.is_only);
        for mut spec in specs {
            if is_only_file { spec.is_only = true; }
            result.push((file_path.clone(), spec));
        }
    }

    if result.iter().any(|(_, spec)| spec.is_only) {
        result.into_iter().filter(|(_, spec)| spec.is_only).collect()
    } else {
        result
    }
}

pub fn get_files_in_dir_recursive(path: &PathBuf) -> Vec<(String, String)> {
    return read_dir_recursively(path);

    fn read_dir_recursively(dir_path: &PathBuf) -> Vec<(String, String)> {
        let mut result = Vec::new();

        for entry in dir_path.read_dir().expect("read dir failed") {
            if let Ok(entry) = entry {
                let entry_path = entry.path();
                if entry_path.is_file() {
                    result.push((entry_path.to_str().unwrap().into(), fs::read_to_string(entry_path).unwrap().into()));
                } else {
                    result.extend(read_dir_recursively(&entry_path));
                }
            }
        }

        result
    }
}
