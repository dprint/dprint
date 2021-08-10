use std::sync::Arc;

use dprint_core::types::ErrBox;

use crate::cache::Cache;
use crate::cli::{TestStdInReader, parse_args, run_cli};
use crate::plugins::{CompilationResult, PluginCache, PluginPools, PluginResolver, PluginsDropper};
use crate::environment::{Environment, TestEnvironment};

// If this file doesn't exist, run `cargo build --release` for crates/test-process-plugin
#[cfg(target_os="windows")]
pub static PROCESS_PLUGIN_EXE_BYTES: &'static [u8] = include_bytes!("../../../target/release/test-process-plugin.exe");
#[cfg(not(target_os="windows"))]
pub static PROCESS_PLUGIN_EXE_BYTES: &'static [u8] = include_bytes!("../../../target/release/test-process-plugin");

// If this file doesn't exist, run `./build.sh` in /crates/test-plugin. (Please consider helping me do something better here :))
pub static WASM_PLUGIN_BYTES: &'static [u8] = include_bytes!("../../test-plugin/target/wasm32-unknown-unknown/release/test_plugin.wasm");
lazy_static! {
    // cache the compilation so this only has to be done once across all tests
    static ref COMPILATION_RESULT: CompilationResult = {
        crate::plugins::compile_wasm(WASM_PLUGIN_BYTES).unwrap()
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
    run_cli(args, environment, &cache, &plugin_resolver, plugin_pools)
}

pub fn get_test_process_plugin_zip_checksum(environment: &TestEnvironment) -> String {
    let plugin_file_bytes = environment.download_file("https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip").unwrap();
    dprint_cli_core::checksums::get_sha256_checksum(&plugin_file_bytes)
}

pub fn get_test_process_plugin_checksum(environment: &TestEnvironment) -> String {
    let plugin_file_bytes = environment.download_file("https://plugins.dprint.dev/test-process.exe-plugin").unwrap();
    dprint_cli_core::checksums::get_sha256_checksum(&plugin_file_bytes)
}

pub fn get_test_wasm_plugin_checksum() -> String {
    dprint_cli_core::checksums::get_sha256_checksum(WASM_PLUGIN_BYTES)
}
