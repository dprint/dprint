use super::*;
use std::fs::{self};
use std::path::Path;
use std::path::PathBuf;

pub fn get_specs_in_dir(path: &Path, parse_spec_options: &ParseSpecOptions) -> Vec<(PathBuf, Spec)> {
  let mut result: Vec<(PathBuf, Spec)> = Vec::new();
  let spec_files = get_files_in_dir_recursive(path);
  for (file_path, text) in spec_files {
    let specs = parse_specs(text, parse_spec_options);
    let lower_case_file_path = file_path.to_string_lossy().to_ascii_lowercase();
    let path_has_only = lower_case_file_path.contains("_only.txt") || lower_case_file_path.contains("_only/") || lower_case_file_path.contains("_only\\");
    let is_only_file = path_has_only && !specs.iter().any(|spec| spec.is_only);
    for mut spec in specs {
      if is_only_file {
        spec.is_only = true;
      }
      result.push((file_path.clone(), spec));
    }
  }

  if result.iter().any(|(_, spec)| spec.is_only) {
    result.into_iter().filter(|(_, spec)| spec.is_only).collect()
  } else {
    result
  }
}

pub fn get_files_in_dir_recursive(path: &Path) -> Vec<(PathBuf, String)> {
  return read_dir_recursively(path);

  fn read_dir_recursively(dir_path: &Path) -> Vec<(PathBuf, String)> {
    let mut result = Vec::new();

    for entry in dir_path.read_dir().expect("read dir failed").flatten() {
      let entry_path = entry.path();
      if entry_path.is_file() {
        let text = fs::read_to_string(&entry_path).unwrap();
        result.push((entry_path, text));
      } else {
        result.extend(read_dir_recursively(&entry_path));
      }
    }

    result
  }
}
