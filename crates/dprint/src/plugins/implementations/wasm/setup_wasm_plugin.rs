use crate::utils::PathSource;
use std::path::Path;

use anyhow::Context;
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
  let is_replacing = environment.path_exists(dest_file_path);
  if let Err(err) = environment.atomic_write_file_bytes(dest_file_path, &compile_result.bytes) {
    // another dprint process having the previous version of the plugin loaded
    // prevents replacing it on Windows, so kill the ones editors start (which
    // they restart) and try again
    let result = if is_replacing && environment.kill_long_running_dprint_processes() > 0 {
      environment.atomic_write_file_bytes(dest_file_path, &compile_result.bytes)
    } else {
      Err(err)
    };
    if let Err(err) = result {
      return if is_replacing && cfg!(windows) {
        Err(err).with_context(|| {
          format!(
            "Failed replacing {}. Maybe another dprint process is using this plugin?",
            dest_file_path.display()
          )
        })
      } else {
        Err(err.into())
      };
    }
  }

  Ok(SetupPluginResult {
    plugin_info: compile_result.plugin_info,
    file_path: dest_file_path.to_path_buf(),
    executable_sub_path: None,
  })
}
