use std::io::Write;
use std::sync::Arc;

use dprint_core::types::ErrBox;

use crate::cache::Cache;
use crate::cli::{parse_args, run_cli};
use crate::environment::TestEnvironment;
use crate::plugins::{CompilationResult, PluginCache, PluginPools, PluginResolver, PluginsDropper};
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

pub fn run_test_cli(args: Vec<&str>, environment: &TestEnvironment) -> Result<(), ErrBox> {
  run_test_cli_with_stdin(args, environment, TestStdInReader::new())
}

pub fn run_test_cli_with_stdin(
  args: Vec<&str>,
  environment: &TestEnvironment,
  stdin_reader: TestStdInReader, // todo: no clue why this can't be passed in by reference
) -> Result<(), ErrBox> {
  let mut args: Vec<String> = args.into_iter().map(String::from).collect();
  args.insert(0, String::from(""));
  environment.set_wasm_compile_result(COMPILATION_RESULT.clone());
  let cache = Arc::new(Cache::new(environment.clone()));
  let plugin_cache = Arc::new(PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(PluginPools::new(environment.clone()));
  let _plugins_dropper = PluginsDropper::new(plugin_pools.clone());
  let plugin_resolver = PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());
  let args = parse_args(args, &stdin_reader)?;
  environment.set_silent(args.is_silent_output());
  environment.set_verbose(args.verbose);
  run_cli(&args, environment, &cache, &plugin_resolver, plugin_pools)
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
}}
}}"#,
    zip_checksum
  )
}
