use anyhow::Result;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;

use super::WasmModuleCreator;
use super::process;
use super::wasm;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginCacheItem;
use crate::plugins::PluginSourceReference;
use crate::utils::PathSource;
use crate::utils::PluginKind;

pub struct SetupPluginResult {
  pub file_path: PathBuf,
  pub plugin_info: PluginInfo,
  /// For process plugins, the executable's path relative to its extract dir.
  /// Stored in the cache meta so the file path can be re-derived on a hit
  /// without re-extracting. `None` for wasm plugins.
  pub executable_sub_path: Option<String>,
}

/// Where a freshly set-up plugin's artifact should be written. Both paths are
/// derived from the plugin's cache hash so the layout stays flat — wasm plugins
/// write a single file and never need a per-plugin directory.
pub struct SetupPluginDest {
  /// File to write the compiled module to (wasm plugins).
  pub wasm_file_path: PathBuf,
  /// Directory to extract into (process plugins).
  pub process_dir_path: PathBuf,
}

pub async fn setup_plugin<TEnvironment: Environment>(
  resolved_source: &PathSource,
  file_bytes: Vec<u8>,
  plugin_kind: PluginKind,
  pre_resolved_tarball: Option<crate::plugins::npm_resolution::PreResolvedProcessPluginTarball>,
  dest: &SetupPluginDest,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  // pass the resolved source to setup functions so process plugins can
  // resolve relative paths in their manifest after a redirect
  match plugin_kind {
    PluginKind::Wasm => wasm::setup_wasm_plugin(resolved_source, file_bytes, &dest.wasm_file_path, environment).await,
    PluginKind::Process => process::setup_process_plugin(resolved_source, &file_bytes, pre_resolved_tarball, &dest.process_dir_path, environment).await,
  }
}

pub async fn create_plugin<TEnvironment: Environment>(
  plugin_cache: &PluginCache<TEnvironment>,
  environment: TEnvironment,
  plugin_reference: &PluginSourceReference,
  wasm_module_creator: &WasmModuleCreator,
) -> Result<Box<dyn Plugin>> {
  let cache_item = match plugin_cache.get_plugin_cache_item(plugin_reference).await {
    Ok(cache_item) => cache_item,
    Err(err) => {
      log_debug!(
        environment,
        "Error getting plugin from cache. Forgetting from cache and retrying. Message: {}",
        err.to_string()
      );

      // forget and try again
      plugin_cache.forget_and_recreate(plugin_reference).await?
    }
  };

  match cache_item.plugin_kind {
    PluginKind::Wasm => {
      // The cached compiled module can fail to read or deserialize (ex. it was
      // compiled for a CPU with different features, or by a different
      // wasm engine/rustc version, or the cache file is corrupt). When that happens,
      // forget the cache, recompile from source, and try once more.
      let plugin = match create_wasm_plugin(&environment, &cache_item, wasm_module_creator) {
        Ok(plugin) => plugin,
        Err(err) => {
          log_debug!(
            environment,
            "Error loading Wasm plugin from cache. Forgetting from cache and retrying. Message: {:#}",
            err
          );

          // forget and try again
          let cache_item = plugin_cache.forget_and_recreate(plugin_reference).await?;
          create_wasm_plugin(&environment, &cache_item, wasm_module_creator)?
        }
      };
      Ok(Box::new(plugin))
    }
    PluginKind::Process => {
      let cache_item = if !environment.path_exists(&cache_item.file_path) {
        log_debug!(
          environment,
          "Could not find process plugin at {}. Forgetting from cache and retrying.",
          cache_item.file_path.display()
        );

        // forget and try again
        plugin_cache.forget_and_recreate(plugin_reference).await?
      } else {
        cache_item
      };

      let executable_path = super::process::get_test_safe_executable_path(&cache_item.info.version, cache_item.file_path, &environment);
      Ok(Box::new(process::ProcessPlugin::new(environment, executable_path, cache_item.info)))
    }
  }
}

/// Reads the cached compiled Wasm module and loads it, verifying it can run on
/// this machine. Returns an error when the cache is unreadable or the module
/// can't be loaded so the caller can recompile from source.
fn create_wasm_plugin<TEnvironment: Environment>(
  environment: &TEnvironment,
  cache_item: &PluginCacheItem,
  wasm_module_creator: &WasmModuleCreator,
) -> Result<wasm::WasmPlugin<TEnvironment>> {
  let file_bytes = environment.read_file_bytes(&cache_item.file_path)?;
  wasm::WasmPlugin::new(&file_bytes, cache_item.info.clone(), wasm_module_creator, environment.clone())
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::test_helpers::WASM_PLUGIN_BYTES;

  // https://github.com/dprint/dprint/issues/734
  #[tokio::test]
  async fn should_recompile_when_cached_wasm_module_fails_to_load() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", WASM_PLUGIN_BYTES);
    let plugin_cache = PluginCache::new(environment.clone());
    let wasm_module_creator = WasmModuleCreator::default();
    let plugin_reference = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");

    // populate the cache (compiles the plugin)
    let cache_item = plugin_cache.get_plugin_cache_item(&plugin_reference).await.unwrap();
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // corrupt the cached compiled module so it can't be deserialized/instantiated
    environment.write_file_bytes(&cache_item.file_path, b"corrupt").unwrap();

    // creating the plugin should recompile from source instead of failing
    let plugin = create_plugin(&plugin_cache, environment.clone(), &plugin_reference, &wasm_module_creator)
      .await
      .unwrap();
    assert_eq!(plugin.info().name, "test-plugin");
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);
  }
}
