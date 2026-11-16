//! Runs the specs in `tests/specs/init`, which each press keys in `dprint init`
//! and say what's on the screen after every one of them and what was written
//! in the end.
//!
//! A spec is a text file made of sections that start with a `-- name --` line:
//!
//! - `-- file: <path> --` is a file to create with the text that follows
//!   (ex. the files of the project, `/dprint.json`, or what was remembered in
//!   `/config/dprint/settings.json`).
//! - `-- remote: <url> --` is a file that can be downloaded (ex. the schema
//!   of a plugin).
//! - `-- info.json --` replaces the plugins that `dprint init` gets to know
//!   about, which are the ones in `add_default_plugins` otherwise.
//! - `-- terminal --` is the size of the terminal as `<cols>x<rows>`.
//! - `-- args --` is the arguments to run, which is `init` otherwise.
//! - `-- keys --` has a step per line. See `ScriptedPromptTerminal::new` for
//!   what a step is.
//! - `-- output --` goes last and is what's expected. Don't write it by hand.
//!   Run the test with the `UPDATE=1` environment variable to have it written
//!   and review the result.
//!
//! Text ahead of the first section says what the spec is about.
//!
//! Only the specs with a name that has the text of the `SPEC` environment
//! variable are run when it's set (ex. `SPEC=resize`).

// the specs are real files, which is what the environment is not for
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use file_test_runner::RunOptions;
use file_test_runner::TestResult;
use file_test_runner::collection::CollectOptions;
use file_test_runner::collection::collect_tests;
use file_test_runner::collection::strategies::TestPerFileCollectionStrategy;

use crate::environment::Environment;
use crate::environment::TestEnvironment;
use crate::environment::TestEnvironmentBuilder;
use crate::environment::TestInfoFileBuilder;
use crate::environment::TestInfoFilePlugin;
use crate::test_helpers::run_test_cli;
use crate::utils::TerminalSize;

#[test]
fn init_specs() {
  let specs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/specs/init");
  let should_update = std::env::var("UPDATE").is_ok_and(|value| value == "1");
  let category = collect_tests(CollectOptions {
    base: specs_dir.clone(),
    strategy: Box::new(TestPerFileCollectionStrategy {
      file_pattern: Some(r"\.txt$".to_string()),
    }),
    // not the filter of `cargo test`, which is for the name of this test
    filter_override: Some(std::env::var("SPEC").unwrap_or_default()),
  })
  .unwrap_or_else(|err| panic!("Failed collecting the specs in {}: {:#}", specs_dir.display(), err));
  assert!(category.test_count() > 0, "No specs matched in {}", specs_dir.display());
  file_test_runner::run_tests(&category, RunOptions::default(), move |test| match run_spec_file(&test.path, should_update) {
    Ok(()) => TestResult::Passed { duration: None },
    Err(err) => TestResult::Failed {
      duration: None,
      output: format!("{:#}\n", err).into_bytes(),
    },
  });
}

const OUTPUT_HEADER: &str = "-- output --";

fn run_spec_file(spec_path: &Path, should_update: bool) -> Result<()> {
  let file_text = std::fs::read_to_string(spec_path)?.replace("\r\n", "\n");
  let (input_text, expected_output) = match file_text.split_once(&format!("{}\n", OUTPUT_HEADER)) {
    Some((input_text, expected_output)) => (input_text, Some(expected_output)),
    None => (file_text.as_str(), None),
  };
  let spec = parse_spec(input_text)?;
  let actual_output = run_spec(&spec)?;
  if expected_output == Some(actual_output.as_str()) {
    return Ok(());
  }
  if should_update {
    let input_text = input_text.trim_end_matches('\n');
    std::fs::write(spec_path, format!("{}\n{}\n{}", input_text, OUTPUT_HEADER, actual_output))?;
    return Ok(());
  }
  match expected_output {
    Some(expected_output) => bail!(
      "The output was not what was expected. Run with UPDATE=1 to update it.\n{}",
      pretty_assertions::StrComparison::new(expected_output, &actual_output)
    ),
    None => bail!("The spec has no output section. Run with UPDATE=1 to add it."),
  }
}

#[derive(Debug, Default, PartialEq)]
struct Spec {
  files: Vec<(String, String)>,
  remote_files: Vec<(String, String)>,
  info_file: Option<String>,
  terminal_size: Option<TerminalSize>,
  args: Vec<String>,
  keys: Vec<String>,
}

fn parse_spec(text: &str) -> Result<Spec> {
  let mut spec = Spec {
    terminal_size: Some(TerminalSize { cols: 80, rows: 24 }),
    args: vec!["init".to_string()],
    ..Default::default()
  };
  let mut sections: Vec<(&str, Vec<&str>)> = Vec::new();
  for line in text.lines() {
    if let Some(name) = line.strip_prefix("-- ").and_then(|line| line.strip_suffix(" --")) {
      sections.push((name.trim(), Vec::new()));
    } else if let Some((_, lines)) = sections.last_mut() {
      lines.push(line);
    }
  }
  for (name, lines) in sections {
    let text = lines.join("\n");
    let non_empty_lines = || lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty());
    if let Some(path) = name.strip_prefix("file:") {
      spec.files.push((path.trim().to_string(), text));
    } else if let Some(url) = name.strip_prefix("remote:") {
      spec.remote_files.push((url.trim().to_string(), text));
    } else {
      match name {
        "info.json" => spec.info_file = Some(text),
        "terminal" => {
          let size = text.trim();
          let parsed_size = size.split_once('x').and_then(|(cols, rows)| Some((cols.parse().ok()?, rows.parse().ok()?)));
          let Some((cols, rows)) = parsed_size else {
            bail!("Expected the terminal size as <cols>x<rows>, but found '{}'.", size);
          };
          spec.terminal_size = Some(TerminalSize { cols, rows });
        }
        "args" => spec.args = text.split_whitespace().map(|arg| arg.to_string()).collect(),
        "keys" => spec.keys = non_empty_lines().map(|line| line.to_string()).collect(),
        _ => bail!("Unknown section '{}'.", name),
      }
    }
  }
  Ok(spec)
}

/// Runs what the spec says and gets everything there is to see of it: the
/// screen after each step, what was logged, and the files it's about.
fn run_spec(spec: &Spec) -> Result<String> {
  let mut builder = TestEnvironmentBuilder::new();
  builder.add_remote_wasm_plugin();
  match &spec.info_file {
    Some(info_file) => {
      builder.add_remote_file("https://plugins.dprint.dev/info.json", info_file);
    }
    None => {
      builder.with_info_file(add_default_plugins);
    }
  }
  for (url, text) in &spec.remote_files {
    builder.add_remote_file(url, text);
  }
  for (path, text) in &spec.files {
    builder.write_file(path, text);
  }
  let environment = builder.build();
  // macOS only uses this directory when it's already there
  environment.mk_dir_all("/config/dprint")?;
  let keys = spec.keys.iter().map(|key| key.as_str()).collect::<Vec<_>>();
  environment.set_prompt_keys(&keys, spec.terminal_size).context("Failed parsing the keys.")?;

  let result = run_test_cli(spec.args.iter().map(|arg| arg.as_str()).collect(), &environment);

  let mut output = String::new();
  for screen in environment.take_prompt_screens() {
    output.push_str(&format!("[{}]\n", screen.step));
    if screen.lines.is_empty() {
      output.push_str("(nothing)\n");
    }
    for line in screen.lines {
      output.push_str(&line);
      output.push('\n');
    }
    output.push('\n');
  }
  if let Err(err) = result {
    err.assert_exit_code(1);
    output.push_str(&format!("# error\n{}\n\n", normalize_paths(&err.to_string())));
  }
  for (name, messages) in [("stdout", environment.take_stdout_messages()), ("stderr", environment.take_stderr_messages())] {
    if !messages.is_empty() {
      output.push_str(&format!("# {}\n{}\n\n", name, normalize_paths(&messages.join("\n"))));
    }
  }
  for path in ["/dprint.json", "/config/dprint/settings.json"] {
    output.push_str(&format!("# {}\n{}\n\n", path, read_file_for_output(&environment, path)));
  }
  let mut output = output.trim_end().to_string();
  output.push('\n');
  // keys that are missing or that nothing asked for are a mistake in the
  // spec and not an outcome
  if environment.is_out_of_prompt_keys() {
    bail!(
      "A prompt was waiting for a key after the last one. What happened up to there:

{}",
      output
    );
  }
  let remaining_keys = environment.remaining_prompt_keys();
  if !remaining_keys.is_empty() {
    bail!(
      "Nothing asked for the keys from '{}' on. What happened up to there:\n\n{}",
      remaining_keys[0],
      output
    );
  }
  Ok(output)
}

/// The plugins a spec gets to choose from when it doesn't bring its own. The
/// test plugin is the one that really exists, so it's what has options.
fn add_default_plugins(info: &mut TestInfoFileBuilder) {
  let plugin = |name: &str, config_key: &str, file_extensions: &[&str]| TestInfoFilePlugin {
    name: format!("dprint-plugin-{}", name),
    version: "1.0.0".to_string(),
    url: format!("https://plugins.dprint.dev/{}-1.0.0.wasm", name),
    config_key: Some(config_key.to_string()),
    file_extensions: file_extensions.iter().map(|ext| ext.to_string()).collect(),
    ..Default::default()
  };
  info
    .add_plugin(plugin("typescript", "typescript", &["ts", "tsx", "js", "jsx"]))
    .add_plugin(plugin("json", "json", &["json", "jsonc"]))
    .add_plugin(plugin("markdown", "markdown", &["md", "mdx"]))
    .add_plugin(plugin("biome", "biome", &["ts", "tsx", "js", "jsx", "json", "jsonc"]))
    .add_plugin(TestInfoFilePlugin {
      name: "test-plugin".to_string(),
      version: "0.2.0".to_string(),
      url: "https://plugins.dprint.dev/test-plugin.wasm".to_string(),
      config_key: Some("test-plugin".to_string()),
      file_extensions: vec!["txt".to_string()],
      ..Default::default()
    });
}

fn read_file_for_output(environment: &TestEnvironment, path: &str) -> String {
  let path = PathBuf::from(path);
  if !environment.path_exists(&path) {
    return "(does not exist)".to_string();
  }
  match environment.read_file(&path) {
    Ok(text) => text.trim_end().to_string(),
    Err(err) => format!("(failed reading: {:#})", err),
  }
}

/// Makes the paths in a message read the same on every platform.
fn normalize_paths(text: &str) -> String {
  text.replace('\\', "/")
}

mod test {
  use super::*;
  use pretty_assertions::assert_eq;

  #[test]
  fn parses_the_sections_of_a_spec() {
    let spec = parse_spec(
      r#"What the spec is about.

-- file: /main.ts --
-- file: /dprint.json --
{
}
-- remote: https://example.com/schema.json --
{}
-- info.json --
{ "latest": [] }
-- terminal --
40x10
-- args --
init --config other.json
-- keys --
down

"some text"
"#,
    )
    .unwrap();
    assert_eq!(
      spec,
      Spec {
        files: vec![("/main.ts".to_string(), "".to_string()), ("/dprint.json".to_string(), "{\n}".to_string())],
        remote_files: vec![("https://example.com/schema.json".to_string(), "{}".to_string())],
        info_file: Some("{ \"latest\": [] }".to_string()),
        terminal_size: Some(TerminalSize { cols: 40, rows: 10 }),
        args: vec!["init".to_string(), "--config".to_string(), "other.json".to_string()],
        keys: vec!["down".to_string(), "\"some text\"".to_string()],
      }
    );
  }

  #[test]
  fn errors_for_what_a_spec_can_not_have() {
    assert_eq!(parse_spec("-- nope --\n").unwrap_err().to_string(), "Unknown section 'nope'.");
    assert_eq!(
      parse_spec("-- terminal --\nbig\n").unwrap_err().to_string(),
      "Expected the terminal size as <cols>x<rows>, but found 'big'."
    );
  }
}
