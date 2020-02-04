use std::collections::HashMap;
use std::path::Path;

use super::*;

struct FailedTestResult {
    file_path: String,
    expected: String,
    actual: String,
    message: String,
}

pub fn run_specs(
    directory_path: &Path,
    parse_spec_options: &ParseSpecOptions,
    format_text: impl Fn(&str, &str, &HashMap<String, String>) -> Result<Option<String>, String>
) {
    let specs = get_specs_in_dir(&directory_path, &parse_spec_options);
    let test_count = specs.len();
    let mut failed_tests = Vec::new();

    for (file_path, spec) in specs.iter().filter(|(_, spec)| !spec.skip) {
        let result = format_text(&spec.file_name, &spec.file_text, &spec.config)
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
}
