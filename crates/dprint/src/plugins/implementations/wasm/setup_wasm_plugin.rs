use crate::utils::PathSource;
use std::path::Path;

use anyhow::Result;

use crate::environment::Environment;

use super::super::SetupPluginResult;

// cache-busting key for the serialized wasmtime artifact. wasmtime additionally
// validates engine/CPU compatibility on deserialize (recompiling on mismatch),
// so this only needs to bump when the wasm engine changes. keep it dot-numeric
// so it parses as a version; tracks the pinned wasmtime version.
pub const WASM_CACHE_VERSION: &str = "43.0.2";

pub async fn setup_wasm_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  file_bytes: Vec<u8>,
  dest_file_path: &Path,
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
  environment.mk_dir_all(dest_file_path.parent().unwrap())?;
  environment.atomic_write_file_bytes(dest_file_path, &compile_result.bytes)?;

  Ok(SetupPluginResult {
    plugin_info: compile_result.plugin_info,
    file_path: dest_file_path.to_path_buf(),
    executable_sub_path: None,
  })
}
