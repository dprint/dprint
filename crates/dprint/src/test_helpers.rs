use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use crossterm::style::Stylize;
use once_cell::sync::Lazy;
use thiserror::Error;

use crate::arg_parser::parse_args;
use crate::environment::TestEnvironment;
use crate::plugins::CompilationResult;
use crate::plugins::PluginCache;
use crate::plugins::PluginResolver;
use crate::plugins::PluginsCollection;
use crate::run_cli::run_cli;
use crate::utils::TestStdInReader;
use crate::AppError;

// this file should automatically be built when building the workspace
pub static TEST_PROCESS_PLUGIN_PATH: Lazy<PathBuf> = Lazy::new(|| {
  let exe_name = if cfg!(windows) { "test-process-plugin.exe" } else { "test-process-plugin" };
  let profile_name = if cfg!(debug_assertions) { "debug" } else { "release" };
  let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
  assert!(target_dir.exists());
  let file_path = target_dir.join(target_dir.join(env!("TARGET"))).join(profile_name).join(exe_name);
  let file_path = if file_path.exists() {
    file_path
  } else {
    target_dir.join(profile_name).join(exe_name)
  };
  std::fs::canonicalize(&file_path).unwrap_or_else(|err| panic!("Could not canonicalize {}: {:#}", file_path.display(), err))
});

// Regenerate this by running `./rebuild.sh` in /crates/test-plugin
pub static WASM_PLUGIN_BYTES: &'static [u8] = include_bytes!("../../test-plugin/test_plugin.wasm");
// cache these so it only has to be done once across all tests
static COMPILATION_RESULT: Lazy<CompilationResult> = Lazy::new(|| crate::plugins::compile_wasm(WASM_PLUGIN_BYTES).unwrap());
pub static PROCESS_PLUGIN_ZIP_BYTES: Lazy<Vec<u8>> = Lazy::new(|| {
  let buf: Vec<u8> = Vec::new();
  let w = std::io::Cursor::new(buf);
  let mut zip = zip::ZipWriter::new(w);
  let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
  zip
    .start_file(
      if cfg!(target_os = "windows") {
        "test-process-plugin.exe"
      } else {
        "test-process-plugin"
      },
      options,
    )
    .unwrap();
  let file_bytes = std::fs::read(&*TEST_PROCESS_PLUGIN_PATH).unwrap();
  zip.write(&file_bytes).unwrap();
  zip.finish().unwrap().into_inner()
});

#[derive(Debug, Error)]
#[error("{inner:#}")]
pub struct TestAppError {
  asserted_exit_code: RefCell<bool>,
  inner: AppError,
}

impl TestAppError {
  #[track_caller]
  pub fn assert_exit_code(&self, exit_code: i32) {
    self.asserted_exit_code.replace(true);
    assert_eq!(self.inner.exit_code, exit_code);
  }
}

impl From<AppError> for TestAppError {
  fn from(inner: AppError) -> Self {
    Self {
      asserted_exit_code: Default::default(),
      inner,
    }
  }
}

impl From<anyhow::Error> for TestAppError {
  fn from(inner: anyhow::Error) -> Self {
    Self {
      asserted_exit_code: Default::default(),
      inner: inner.into(),
    }
  }
}

impl Drop for TestAppError {
  fn drop(&mut self) {
    if std::thread::panicking() || self.inner.exit_code <= 1 {
      return;
    }
    if !self.asserted_exit_code.borrow().clone() {
      panic!("Exit code must be asserted. Was: {}", self.inner.exit_code);
    }
  }
}

pub fn run_test_cli(args: Vec<&str>, environment: &TestEnvironment) -> Result<(), TestAppError> {
  run_test_cli_with_stdin(args, environment, TestStdInReader::default())
}

pub fn run_test_cli_with_stdin(args: Vec<&str>, environment: &TestEnvironment, stdin_reader: TestStdInReader) -> Result<(), TestAppError> {
  let mut args: Vec<String> = args.into_iter().map(String::from).collect();
  args.insert(0, String::from(""));
  environment.set_wasm_compile_result(COMPILATION_RESULT.clone());
  let plugin_cache = Arc::new(PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(PluginsCollection::new(environment.clone()));
  let plugin_resolver = PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());
  let args = parse_args(args, stdin_reader).map_err(|err| Into::<AppError>::into(err))?;
  environment.set_stdout_machine_readable(args.is_stdout_machine_readable());
  environment.set_verbose(args.verbose);

  environment.run_in_runtime({
    let environment = environment.clone();
    async move {
      let result = run_cli(&args, &environment, &plugin_resolver, plugin_pools.clone()).await;
      plugin_pools.drop_and_shutdown_initialized().await;
      Ok(result?)
    }
  })
}

pub fn get_test_process_plugin_zip_checksum() -> String {
  crate::utils::get_sha256_checksum(&PROCESS_PLUGIN_ZIP_BYTES)
}

pub fn get_test_wasm_plugin_checksum() -> String {
  crate::utils::get_sha256_checksum(WASM_PLUGIN_BYTES)
}

pub fn get_test_process_plugin_checksum() -> String {
  let zip_checksum = get_test_process_plugin_zip_checksum();
  let ps_file_bytes = get_test_process_plugin_file_text(&zip_checksum).into_bytes();
  crate::utils::get_sha256_checksum(&ps_file_bytes)
}

pub fn get_test_process_plugin_file_text(zip_checksum: &str) -> String {
  format!(
    r#"{{
"schemaVersion": 2,
"name": "test-process-plugin",
"version": "0.1.0",
"windows-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"linux-aarch64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"linux-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"darwin-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"darwin-aarch64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}}
}}"#,
    zip_checksum
  )
}

pub fn get_singular_formatted_text() -> String {
  format!("Formatted {} file.", "1".bold().to_string())
}

pub fn get_plural_formatted_text(count: usize) -> String {
  format!("Formatted {} files.", count.to_string().bold().to_string())
}

pub fn get_singular_check_text() -> String {
  format!("Found {} not formatted file.", "1".bold().to_string())
}

pub fn get_plural_check_text(count: usize) -> String {
  format!("Found {} not formatted files.", count.to_string().bold().to_string())
}

pub fn get_expected_help_text() -> &'static str {
  concat!(
    "dprint ",
    env!("CARGO_PKG_VERSION"),
    r#"
Copyright 2020-2023 by David Sherret

Auto-formats source code based on the specified plugins.

USAGE:
    dprint <SUBCOMMAND> [OPTIONS] [--] [file patterns]...

SUBCOMMANDS:
  init                    Initializes a configuration file in the current directory.
  fmt                     Formats the source files and writes the result to the file system.
  check                   Checks for any files that haven't been formatted.
  config                  Functionality related to the configuration file.
  output-file-paths       Prints the resolved file paths for the plugins based on the args and configuration.
  output-resolved-config  Prints the resolved configuration for the plugins based on the args and configuration.
  output-format-times     Prints the amount of time it takes to format each file. Use this for debugging.
  clear-cache             Deletes the plugin cache directory.
  upgrade                 Upgrades the dprint executable.
  license                 Outputs the software license.

More details at `dprint help <SUBCOMMAND>`

OPTIONS:
  -c, --config <config>          Path or url to JSON configuration file. Defaults to dprint.json(c) or .dprint.json(c) in current or ancestor directory when not provided.
      --plugins <urls/files>...  List of urls or file paths of plugins to use. This overrides what is specified in the config file.
      --verbose                  Prints additional diagnostic information.

ENVIRONMENT VARIABLES:
  DPRINT_CACHE_DIR    Directory to store the dprint cache. Note that this
                      directory may be periodically deleted by the CLI.
  DPRINT_MAX_THREADS  Limit the number of threads dprint uses for
                      formatting (ex. DPRINT_MAX_THREADS=4).
  HTTPS_PROXY         Proxy to use when downloading plugins or configuration
                      files (set HTTP_PROXY for HTTP).

GETTING STARTED:
  1. Navigate to the root directory of a code repository.
  2. Run `dprint init` to create a dprint.json file in that directory.
  3. Modify configuration file if necessary.
  4. Run `dprint fmt` or `dprint check`.

EXAMPLES:
  Write formatted files to file system:

    dprint fmt

  Check for files that haven't been formatted:

    dprint check

  Specify path to config file other than the default:

    dprint fmt --config path/to/config/dprint.json

  Search for files using the specified file patterns:

    dprint fmt "**/*.{ts,tsx,js,jsx,json}"
"#
  )
}
