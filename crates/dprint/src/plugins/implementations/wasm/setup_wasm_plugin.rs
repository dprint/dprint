use crate::utils::PathSource;
use std::path::PathBuf;

use anyhow::Result;
use dprint_core::plugins::PluginInfo;

use crate::environment::Environment;

use super::super::SetupPluginResult;

pub const WASMER_COMPILER_VERSION: &str = wasmer_compiler::VERSION;

pub fn get_file_path_from_plugin_info(plugin_info: &PluginInfo, environment: &impl Environment) -> PathBuf {
  let cache_dir_path = environment.get_cache_dir();
  let plugin_cache_dir_path = cache_dir_path.join("plugins").join(&plugin_info.name);
  // this is keyed on both the wasmer compiler version and system cache key
  plugin_cache_dir_path.join(format!("{}-{}-{}", plugin_info.version, WASMER_COMPILER_VERSION, environment.wasm_cache_key()))
}

pub async fn setup_wasm_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  file_bytes: Vec<u8>,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let guard = environment
    .progress_bars()
    .map(|pb| pb.add_progress(format!("Compiling {}", url_or_file_path.display()), crate::utils::ProgressBarStyle::Action, 1));
  if guard.is_none() {
    log_stderr_info!(environment, "Compiling {}", url_or_file_path.display());
  }
  let compile_result = dprint_core::async_runtime::spawn_blocking({
    let environment = environment.clone();
    move || environment.compile_wasm(&file_bytes)
  })
  .await??;
  drop(guard);
  let plugin_info = compile_result.plugin_info;
  let plugin_cache_file_path = get_file_path_from_plugin_info(&plugin_info, environment);
  environment.mk_dir_all(plugin_cache_file_path.parent().unwrap())?;
  environment.atomic_write_file_bytes(&plugin_cache_file_path, &compile_result.bytes)?;

  Ok(SetupPluginResult {
    plugin_info,
    file_path: plugin_cache_file_path,
  })
}

pub fn cleanup_wasm_plugin(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<()> {
  let plugin_file_path = get_file_path_from_plugin_info(plugin_info, environment);
  environment.remove_file(plugin_file_path)?;
  Ok(())
}
