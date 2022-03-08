use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use crossterm::style::Stylize;

use crate::arg_parser::parse_args;
use crate::cache::Cache;
use crate::environment::TestEnvironment;
use crate::plugins::CompilationResult;
use crate::plugins::PluginCache;
use crate::plugins::PluginPools;
use crate::plugins::PluginResolver;
use crate::plugins::PluginsDropper;
use crate::run_cli::run_cli;
use crate::utils::TestStdInReader;

// If this file doesn't exist, run `cargo build --release` for crates/test-process-plugin
#[cfg(target_os = "windows")]
pub static PROCESS_PLUGIN_EXE_BYTES: &'static [u8] = include_bytes!("../../../target/release/test-process-plugin.exe");
#[cfg(not(target_os = "windows"))]
pub static PROCESS_PLUGIN_EXE_BYTES: &'static [u8] = include_bytes!("../../../target/release/test-process-plugin");

// If this file doesn't exist, run `./build.sh` in /crates/test-plugin. (Please consider helping me do something better here :))
pub static WASM_PLUGIN_BYTES: &'static [u8] = include_bytes!("../../test-plugin/target/wasm32-unknown-unknown/release/test_plugin.wasm");
// cache these so it only has to be done once across all tests
lazy_static! {
  static ref COMPILATION_RESULT: CompilationResult = crate::plugins::compile_wasm(WASM_PLUGIN_BYTES).unwrap();
}
lazy_static! {
  pub static ref PROCESS_PLUGIN_ZIP_BYTES: Vec<u8> = {
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
    zip.write(PROCESS_PLUGIN_EXE_BYTES).unwrap();
    zip.finish().unwrap().into_inner()
  };
}

pub fn run_test_cli(args: Vec<&str>, environment: &TestEnvironment) -> Result<()> {
  run_test_cli_with_stdin(args, environment, TestStdInReader::default())
}

pub fn run_test_cli_with_stdin(args: Vec<&str>, environment: &TestEnvironment, stdin_reader: TestStdInReader) -> Result<()> {
  let mut args: Vec<String> = args.into_iter().map(String::from).collect();
  args.insert(0, String::from(""));
  environment.set_wasm_compile_result(COMPILATION_RESULT.clone());
  let cache = Arc::new(Cache::new(environment.clone()));
  let plugin_cache = Arc::new(PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(PluginPools::new(environment.clone()));
  let _plugins_dropper = PluginsDropper::new(plugin_pools.clone());
  let plugin_resolver = PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());
  let args = parse_args(args, stdin_reader)?;
  environment.set_stdout_machine_readable(args.is_stdout_machine_readable());
  environment.set_verbose(args.verbose);

  let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
  let environment = environment.clone();
  rt.block_on(async move { run_cli(&args, &environment, &cache, &plugin_resolver, plugin_pools).await })
}

pub fn get_test_process_plugin_zip_checksum() -> String {
  dprint_cli_core::checksums::get_sha256_checksum(&PROCESS_PLUGIN_ZIP_BYTES)
}

pub fn get_test_wasm_plugin_checksum() -> String {
  dprint_cli_core::checksums::get_sha256_checksum(WASM_PLUGIN_BYTES)
}

pub fn get_test_process_plugin_checksum() -> String {
  let zip_checksum = get_test_process_plugin_zip_checksum();
  let ps_file_bytes = get_test_process_plugin_file_text(&zip_checksum).into_bytes();
  dprint_cli_core::checksums::get_sha256_checksum(&ps_file_bytes)
}

pub fn get_test_process_plugin_file_text(zip_checksum: &str) -> String {
  format!(
    r#"{{
"schemaVersion": 1,
"name": "test-process-plugin",
"version": "0.1.0",
"windows-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"linux-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"mac-x86_64": {{
    "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
    "checksum": "{0}"
}},
"mac-aarch64": {{
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
Copyright 2020-2022 by David Sherret

Auto-formats source code based on the specified plugins.

USAGE:
    dprint <SUBCOMMAND> [OPTIONS] [--] [file patterns]...

SUBCOMMANDS:
    init                      Initializes a configuration file in the current directory.
    fmt                       Formats the source files and writes the result to the file system.
    check                     Checks for any files that haven't been formatted.
    config                    Functionality related to the configuration file.
    output-file-paths         Prints the resolved file paths for the plugins based on the args and configuration.
    output-resolved-config    Prints the resolved configuration for the plugins based on the args and configuration.
    output-format-times       Prints the amount of time it takes to format each file. Use this for debugging.
    clear-cache               Deletes the plugin cache directory.
    license                   Outputs the software license.

More details at `dprint help <SUBCOMMAND>`

OPTIONS:
    -c, --config <config>            Path or url to JSON configuration file. Defaults to dprint.json or .dprint.json in
                                     current or ancestor directory when not provided.
        --plugins <urls/files>...    List of urls or file paths of plugins to use. This overrides what is specified in
                                     the config file.
        --verbose                    Prints additional diagnostic information.
    -v, --version                    Prints the version.

ENVIRONMENT VARIABLES:
  DPRINT_CACHE_DIR    The directory to store the dprint cache. Note that
                      this directory may be periodically deleted by the CLI.

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

    dprint fmt "**/*.{ts,tsx,js,jsx,json}""#
  )
}
