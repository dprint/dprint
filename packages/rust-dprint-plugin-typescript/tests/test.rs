extern crate dprint_plugin_typescript;
extern crate dprint_development;

#[macro_use] extern crate debug_here;

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

fn test_performance() {
    // run this with `cargo test --release -- --nocapture`

    // todo: fix up this javascript era code
    let mut unresolved_config = HashMap::new();
    // This file was not written with an 80 line width in mind so overall
    // it's not too bad, but there are a few small issues to fix here and there.
    unresolved_config.insert(String::from("lineWidth"), String::from("80"));
    unresolved_config.insert(String::from("forceMultiLineArguments"), String::from("true"));
    unresolved_config.insert(String::from("forceMultiLineParameters"), String::from("true"));
    unresolved_config.insert(String::from("singleQuotes"), String::from("true"));
    unresolved_config.insert(String::from("nextControlFlowPosition"), String::from("sameLine"));
    let mut diagnostics = Vec::new();

    let config = resolve_config(&unresolved_config, &mut diagnostics);
    let file_text = fs::read_to_string("tests/performance/checker.txt").expect("Expected to read.");

    //debug_here!();

    for i in 0..10 {
        let start = Instant::now();
        let result = format_text("checker.ts", &file_text, &config).expect("Could not parse...");
        let result = if let Some(result) = result { result } else { file_text.clone() };

        let elapsed = start.elapsed();
        println!("{}ms", elapsed.as_millis());
        println!("---");

        if i == 0 {
            fs::write("tests/performance/checker_output.txt", result).expect("Expected to write to the file.");
        }
    }
}

#[test]
fn test_specs() {
    let specs = get_specs();
    let test_count = specs.len();
    let mut failed_tests = Vec::new();

    for (file_path, spec) in specs.iter().filter(|(_, spec)| !spec.skip) {
        let mut diagnostics = Vec::new();
        let config = resolve_config(&spec.config, &mut diagnostics);
        ensure_no_diagnostics(&diagnostics);

        let result = format_text(&spec.file_name, &spec.file_text, &config)
            .expect(format!("Could not parse spec '{}' in {}", spec.message, file_path).as_str());
        let result = if let Some(result) = result { result } else { spec.file_text.clone() };
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

    fn ensure_no_diagnostics(diagnostics: &Vec<ConfigurationDiagnostic>) {
        for diagnostic in diagnostics {
            panic!("Diagnostic error for '{}': {}", diagnostic.property_name, diagnostic.message);
        }
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
