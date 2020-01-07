extern crate dprint_plugin_typescript;
extern crate dprint_development;

use dprint_plugin_typescript::*;
use dprint_development::*;
use std::fs::{self};
use std::path::Path;
use std::time::Instant;
use std::collections::HashMap;

struct FailedTestResult {
    file_path: String,
    expected: String,
    actual: String,
    message: String,
}

#[test]
fn test_performance() {
    let start = Instant::now();
    let config = resolve_config(&HashMap::new());

    let file_text = fs::read_to_string("V:\\performance-test\\files\\checker.ts").expect("Expected to read.");
    let result = format_text("V:\\checker.ts", &file_text, &config).expect("Could not parse...");

    let elapsed = start.elapsed();
    println!("Finished in {}ms", elapsed.as_millis());
    println!("Length: {}", result.len());
}

fn test_specs() {
    let specs = get_specs();
    let test_count = specs.len();
    let mut failed_tests = Vec::new();

    for (file_path, spec) in specs.iter().filter(|(_, spec)| !spec.skip) {
        let config = resolve_config(&spec.config);
        let result = format_text(&spec.file_name, &spec.file_text, &config).expect(format!("Could not parse spec '{}' in {}", spec.message, file_path).as_str());
        if result != spec.expected_text {
            failed_tests.push(FailedTestResult {
                file_path: file_path.clone(),
                expected: spec.expected_text.clone(),
                actual: result,
                message: spec.message.clone()
            });
        }
    }

    for failed_test in &failed_tests {
        println!("---");
        println!("Failed:   {} ({})\nExpected: `{:?}`,\nActual:   `{:?}`", failed_test.message, failed_test.file_path, failed_test.expected, failed_test.actual);
    }

    if !failed_tests.is_empty() {
        println!("---");
        panic!("{}/{} tests passed", test_count - failed_tests.len(), test_count);
    }
}

fn get_specs() -> Vec<(String, Spec)> {
    let mut result: Vec<(String, Spec)> = Vec::new();
    let spec_files = get_spec_files();
    for (file_path, text) in spec_files {
        let specs = parse_specs(text, ParseSpecOptions { default_file_name: "file.ts" });
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

fn get_spec_files() -> Vec<(String, String)> {
    return read_dir_recursively(&Path::new("./tests/specs"));

    fn read_dir_recursively(dir_path: &Path) -> Vec<(String, String)> {
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
