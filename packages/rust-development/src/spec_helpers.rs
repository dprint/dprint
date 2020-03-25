use std::collections::HashMap;
use std::path::Path;
use std::fs::{self};

use super::*;

struct FailedTestResult {
    file_path: String,
    expected: String,
    actual: String,
    actual_second: Option<String>,
    message: String,
}

pub struct RunSpecsOptions {
    /// Set to true to overwrite the failing tests with the actual result.
    pub fix_failures: bool,
}

pub fn run_specs(
    directory_path: &Path,
    parse_spec_options: &ParseSpecOptions,
    run_spec_options: &RunSpecsOptions,
    format_text: impl Fn(&str, &str, &HashMap<String, String>) -> Result<Option<String>, String>
) {
    #[cfg(not(debug_assertions))]
    assert_not_fix_failures(run_spec_options);

    let specs = get_specs_in_dir(&directory_path, &parse_spec_options);
    let test_count = specs.len();
    let mut failed_tests = Vec::new();

    for (file_path, spec) in specs.iter().filter(|(_, spec)| !spec.skip) {
        #[cfg(not(debug_assertions))]
        assert_spec_not_only(&spec);

        let result = format_text(&spec.file_name, &spec.file_text, &spec.config)
            .expect(format!("Could not parse spec '{}' in {}", spec.message, file_path).as_str());
        let result = if let Some(result) = result { result } else { spec.file_text.clone() };
        if result != spec.expected_text {
            if run_spec_options.fix_failures {
                // very rough, but good enough
                let file_path = Path::new(file_path);
                let file_text = fs::read_to_string(file_path).expect("Expected to read the file.");
                let file_text = file_text.replace(&spec.expected_text.replace("\n", "\r\n"), &result.replace("\n", "\r\n"));
                fs::write(file_path, file_text).expect("Expected to write to file.");
            } else {
                failed_tests.push(FailedTestResult {
                    file_path: file_path.clone(),
                    expected: spec.expected_text.clone(),
                    actual: result,
                    actual_second: None,
                    message: spec.message.clone()
                });
            }
        } else {
            // todo: re-enable
            /*
            // ensure no changes when formatting twice
            let twice_result = format_text(&spec.file_name, &result, &spec.config)
                .expect(format!(
                    "Could not parse when formatting twice '{}' in {} for text {}",
                    spec.message,
                    file_path,
                    result
                ).as_str());
            let twice_result = if let Some(twice_result) = twice_result { twice_result } else { spec.file_text.clone() };
            if twice_result != spec.expected_text {
                failed_tests.push(FailedTestResult {
                    file_path: file_path.clone(),
                    expected: spec.expected_text.clone(),
                    actual: result,
                    actual_second: Some(twice_result),
                    message: spec.message.clone()
                });
            }
            */
        }
    }

    for failed_test in &failed_tests {
        println!("---");
        let mut failed_message = format!(
            "Failed:   {} ({})\nExpected: `{:?}`,\nActual:   `{:?}`,`",
            failed_test.message,
            failed_test.file_path,
            failed_test.expected,
            failed_test.actual,
        );
        if let Some(actual_second) = &failed_test.actual_second {
            failed_message.push_str(&format!(
                "\nTwice:    `{:?}`",
                actual_second
            ));
        }
        println!("{}", failed_message);
    }

    if !failed_tests.is_empty() {
        println!("---");
        panic!("{}/{} tests passed", test_count - failed_tests.len(), test_count);
    }

    #[cfg(not(debug_assertions))]
    fn assert_spec_not_only(spec: &Spec) {
        if spec.is_only {
            panic!("Cannot run 'only' spec in release mode: {}", spec.message);
        }
    }

    #[cfg(not(debug_assertions))]
    fn assert_not_fix_failures(run_spec_options: &RunSpecsOptions) {
        if run_spec_options.fix_failures {
            panic!("Cannot have 'fix_failures' as `true` in release mode.");
        }
    }
}
