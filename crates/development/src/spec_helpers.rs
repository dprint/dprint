use anyhow::Result;
use console::Style;
use file_test_runner::RunOptions;
use file_test_runner::SubTestResult;
use file_test_runner::TestResult;
use file_test_runner::collection::CollectOptions;
use similar::ChangeTag;
use similar::TextDiff;
use std::fmt::Display;
use std::fs;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;

struct FailedTestResult {
  expected: String,
  actual: String,
  actual_second: Option<String>,
  message: String,
}

struct DiffFailedMessage<'a> {
  expected: &'a str,
  actual: &'a str,
}

impl Display for DiffFailedMessage<'_> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let diff = TextDiff::from_lines(self.expected, self.actual);

    for op in diff.ops() {
      for change in diff.iter_changes(op) {
        let (sign, style) = match change.tag() {
          ChangeTag::Delete => ("-", Style::new().green()),
          ChangeTag::Insert => ("+", Style::new().red()),
          ChangeTag::Equal => (" ", Style::new()),
        };
        write!(f, "{}{}", style.apply_to(sign).bold(), style.apply_to(change),)?;
      }
    }
    Ok(())
  }
}

type FormatTextFunc = dyn (Fn(&Path, &str, &SpecConfigMap) -> Result<Option<String>>) + Send + Sync;
type GetTraceJsonFunc = dyn (Fn(&Path, &str, &SpecConfigMap) -> String) + Send + Sync;

#[derive(Debug, Clone)]
pub struct RunSpecsOptions {
  /// Set to true to overwrite the failing tests with the actual result.
  pub fix_failures: bool,
  pub format_twice: bool,
}

pub fn run_specs(
  directory_path: &Path,
  parse_spec_options: &ParseSpecOptions,
  run_spec_options: &RunSpecsOptions,
  format_text: Arc<FormatTextFunc>,
  get_trace_json: Arc<GetTraceJsonFunc>,
) {
  #[cfg(not(debug_assertions))]
  assert_not_fix_failures(run_spec_options);

  let parse_spec_options = parse_spec_options.clone();
  let run_spec_options = run_spec_options.clone();
  file_test_runner::collect_and_run_tests(
    CollectOptions {
      base: directory_path.to_path_buf(),
      filter_override: None,
      strategy: Box::new(file_test_runner::collection::strategies::TestPerFileCollectionStrategy { file_pattern: None }),
    },
    RunOptions { parallel: true },
    Arc::new(move |test| {
      let file_text = test.read_to_string().unwrap();
      let specs = parse_specs(file_text, &parse_spec_options);
      let specs = if specs.iter().any(|s| s.is_only) {
        specs.into_iter().filter(|s| s.is_only).collect()
      } else {
        specs
      };
      let mut sub_tests = Vec::new();
      for spec in specs {
        #[cfg(not(debug_assertions))]
        assert_spec_not_only_or_trace(&spec);

        if spec.skip {
          sub_tests.push(SubTestResult {
            name: spec.message.clone(),
            result: TestResult::Ignored,
          });
          continue;
        }

        let test_file_path = &test.path;
        let maybe_failed_result = run_spec(&spec, test_file_path, &run_spec_options, &format_text, &get_trace_json);

        sub_tests.push(SubTestResult {
          name: spec.message.clone(),
          result: if let Some(failed_test) = maybe_failed_result {
            let mut output = Vec::<u8>::new();
            let mut failed_message = format!(
              "Failed:   {} ({})\nExpected: `{:?}`,\nActual:   `{:?}`,`,\nDiff:\n{}",
              failed_test.message,
              test_file_path.display(),
              failed_test.expected,
              failed_test.actual,
              DiffFailedMessage {
                actual: &failed_test.actual,
                expected: &failed_test.expected
              }
            );
            if let Some(actual_second) = &failed_test.actual_second {
              failed_message.push_str(&format!(
                "\nTwice:    `{:?}`,\nTwice diff:\n{}",
                actual_second,
                DiffFailedMessage {
                  actual: actual_second,
                  expected: &failed_test.actual,
                }
              ));
            }
            output.extend(failed_message.as_bytes());
            TestResult::Failed { output }
          } else {
            TestResult::Passed
          },
        });
      }

      TestResult::SubTests(sub_tests)
    }),
  );

  fn run_spec(
    spec: &Spec,
    test_file_path: &Path,
    run_spec_options: &RunSpecsOptions,
    format_text: &Arc<FormatTextFunc>,
    get_trace_json: &Arc<GetTraceJsonFunc>,
  ) -> Option<FailedTestResult> {
    let spec_file_path_buf = PathBuf::from(&spec.file_name);
    let format = |file_text: &str| -> Result<Option<String>, String> {
      match catch_unwind(AssertUnwindSafe(|| format_text(&spec_file_path_buf, file_text, &spec.config))) {
        Ok(Ok(formatted)) => Ok(formatted),
        Ok(Err(err)) => Err(format!("Formatter error: {:#}", err)),
        Err(panic_info) => {
          let panic_msg = panic_info.downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| panic_info.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
          Err(format!("Formatter panicked: {}", panic_msg))
        }
      }
    };

    if spec.is_trace {
      let trace_json = get_trace_json(&spec_file_path_buf, &spec.file_text, &spec.config);
      handle_trace(spec, &trace_json);
      None
    } else {
      let result = match format(&spec.file_text) {
        Ok(formatted) => formatted.unwrap_or_else(|| spec.file_text.to_string()),
        Err(err_msg) => {
          return Some(FailedTestResult {
            expected: spec.expected_text.clone(),
            actual: format!("{}\n\nInput:\n{}", err_msg, spec.file_text),
            actual_second: None,
            message: spec.message.clone(),
          });
        }
      };

      if result != spec.expected_text {
        if run_spec_options.fix_failures {
          // very rough, but good enough
          let file_text = fs::read_to_string(test_file_path).expect("Expected to read the file.");
          let file_text = file_text.replace(&spec.expected_text, &result);
          fs::write(test_file_path, file_text).expect("Expected to write to file.");
          None
        } else {
          Some(FailedTestResult {
            expected: spec.expected_text.clone(),
            actual: result,
            actual_second: None,
            message: spec.message.clone(),
          })
        }
      } else if run_spec_options.format_twice && !spec.skip_format_twice {
        // ensure no changes when formatting twice
        let twice_result = match format(&result) {
          Ok(formatted) => formatted.unwrap_or_else(|| result.to_string()),
          Err(err_msg) => {
            return Some(FailedTestResult {
              expected: spec.expected_text.clone(),
              actual: result,
              actual_second: Some(format!("ERROR on second format: {}", err_msg)),
              message: spec.message.clone(),
            });
          }
        };
        if twice_result != spec.expected_text {
          Some(FailedTestResult {
            expected: spec.expected_text.clone(),
            actual: result,
            actual_second: Some(twice_result),
            message: spec.message.clone(),
          })
        } else {
          None
        }
      } else {
        None
      }
    }
  }

  fn handle_trace(spec: &Spec, trace_json: &str) {
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
    script.push_str(&format!("const specMessage = \"{}\";\n", spec.message.replace('"', "\\\"")));
    script.push_str(app_js_text);
    let html_file = html_file
      .replace("<!-- script -->", &script)
      .replace("<!-- title -->", &format!("Trace - {}", spec.message))
      .replace("<!-- style -->", app_css_text);
    let temp_file_path = std::env::temp_dir().join("dprint-core-trace.html");
    fs::write(&temp_file_path, html_file).unwrap();
    let url = format!("file://{}", temp_file_path.to_string_lossy().replace('\\', "/"));
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
