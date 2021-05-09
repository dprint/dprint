use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs::{self};
use std::fmt::Display;

use console::Style;
use similar::text::{ChangeTag, TextDiff};
use super::*;

struct FailedTestResult {
    file_path: String,
    expected: String,
    actual: String,
    actual_second: Option<String>,
    message: String,
}

struct DiffFailedMessage<'a> {
    expected: &'a str,
    actual: &'a str
}

impl<'a> Display for DiffFailedMessage<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diff = TextDiff::from_lines(self.expected, self.actual);

        for op in diff.ops() {
            for change in diff.iter_changes(op) {
                let (sign, style) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::new().green()),
                    ChangeTag::Insert => ("+", Style::new().red()),
                    ChangeTag::Equal => (" ", Style::new()),
                };
                write!(
                    f,
                    "{}{}",
                    style.apply_to(sign).bold(),
                    style.apply_to(change),
                )?;
            }
        }
        Ok(())
    }
}

pub struct RunSpecsOptions {
    /// Set to true to overwrite the failing tests with the actual result.
    pub fix_failures: bool,
    pub format_twice: bool,
}

pub fn run_specs(
    directory_path: &Path,
    parse_spec_options: &ParseSpecOptions,
    run_spec_options: &RunSpecsOptions,
    format_text: impl Fn(&Path, &str, &HashMap<String, String>) -> Result<String, String>,
    get_trace_json: impl Fn(&Path, &str, &HashMap<String, String>) -> String,
) {
    #[cfg(not(debug_assertions))]
    assert_not_fix_failures(run_spec_options);

    let specs = get_specs_in_dir(&directory_path, &parse_spec_options);
    let test_count = specs.len();
    let mut failed_tests = Vec::new();

    for (file_path, spec) in specs.into_iter().filter(|(_, spec)| !spec.skip) {
        #[cfg(not(debug_assertions))]
        assert_spec_not_only_or_trace(&spec);

        let file_path_buf = PathBuf::from(&spec.file_name);
        let format = |file_text: &str| {
            format_text(&file_path_buf, &file_text, &spec.config)
                .unwrap_or_else(|_| panic!("Could not parse spec '{}' in {}", spec.message, file_path))
        };

        if spec.is_trace {
            let trace_json = get_trace_json(&file_path_buf, &spec.file_text, &spec.config);
            handle_trace(&spec, &trace_json);
        } else {
            let result = format(&spec.file_text);
            if result != spec.expected_text {
                if run_spec_options.fix_failures {
                    // very rough, but good enough
                    let file_path = PathBuf::from(&file_path);
                    let file_text = fs::read_to_string(&file_path).expect("Expected to read the file.");
                    let file_text = file_text.replace(&spec.expected_text.replace("\n", "\r\n"), &result.replace("\n", "\r\n"));
                    fs::write(&file_path, file_text).expect("Expected to write to file.");
                } else {
                    failed_tests.push(FailedTestResult {
                        file_path: file_path.clone(),
                        expected: spec.expected_text.clone(),
                        actual: result,
                        actual_second: None,
                        message: spec.message.clone()
                    });
                }
            } else if run_spec_options.format_twice && !spec.skip_format_twice {
                // ensure no changes when formatting twice
                let twice_result = format(&result);
                if twice_result != spec.expected_text {
                    failed_tests.push(FailedTestResult {
                        file_path: file_path.clone(),
                        expected: spec.expected_text.clone(),
                        actual: result,
                        actual_second: Some(twice_result),
                        message: spec.message.clone()
                    });
                }
            }
        }
    }

    for failed_test in &failed_tests {
        println!("---");
        let mut failed_message = format!(
            "Failed:   {} ({})\nExpected: `{:?}`,\nActual:   `{:?}`,`,\nDiff:\n{}",
            failed_test.message,
            failed_test.file_path,
            failed_test.expected,
            failed_test.actual,
            DiffFailedMessage {
                actual: &failed_test.actual,
                expected:&failed_test.expected
            }
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

    fn handle_trace(
        spec: &Spec,
        trace_json: &str,
    ) {
        let app_js_text = include_str!("../trace_analyzer/app.js");
        let app_css_text = include_str!("../trace_analyzer/app.css");
        let html_file = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width">
    <title><!-- title --></title>
    <script src="https://d3js.org/d3.v5.min.js"></script>
    <script src="https://d3js.org/d3-quadtree.v1.min.js"></script>
    <script src="https://d3js.org/d3-timer.v1.min.js"></script>
    <script src="https://d3js.org/d3-force.v2.min.js"></script>
    <script src="https://d3js.org/d3-color.v2.min.js"></script>
    <script src="https://d3js.org/d3-dispatch.v2.min.js"></script>
    <script src="https://d3js.org/d3-ease.v2.min.js"></script>
    <script src="https://d3js.org/d3-interpolate.v2.min.js"></script>
    <script src="https://d3js.org/d3-selection.v2.min.js"></script>
    <script src="https://d3js.org/d3-timer.v2.min.js"></script>
    <script src="https://d3js.org/d3-transition.v2.min.js"></script>
    <script src="https://d3js.org/d3-drag.v2.min.js"></script>
    <script src="https://d3js.org/d3-zoom.v2.min.js"></script>
    <script type="text/javascript">
    <!-- script -->
    </script>
    <style>
    <!-- style -->
    </style>
</head>
<body onload="onLoad()">
</body>
</html>"#;
        let mut script = format!("const rawTraceResult = {};\n", trace_json);
        script.push_str(&format!("const specMessage = \"{}\";\n", spec.message.replace("\"", "\\\"")));
        script.push_str(app_js_text);
        let html_file = html_file
            .replace("<!-- script -->", &script)
            .replace("<!-- title -->", &format!("Trace - {}", spec.message))
            .replace("<!-- style -->", app_css_text);
        let temp_file_path = std::env::temp_dir().join("dprint-core-trace.html");
        fs::write(&temp_file_path, html_file).unwrap();
        let url = format!("file://{}", temp_file_path.to_string_lossy().replace("\\", "/"));
        panic!("\n==============\nTrace output ready! Please open your browser to: {}\n==============\n", url);
    }

    #[cfg(not(debug_assertions))]
    fn assert_spec_not_only_or_trace(spec: &Spec) {
        if spec.is_trace {
            panic!("Cannot run 'trace' spec in release mode: {}", spec.message);
        }

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
