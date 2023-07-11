use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::process::ProcessPluginExecutableInfo;
use std::path::PathBuf;
use std::sync::Arc;

use dprint_core::plugins::PluginInfo;

use super::process;
use super::process::resolve_node_executable;
use super::wasm;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginsCollection;

use crate::plugins::cache_manifest::PluginCacheManifestItem;
use crate::utils::PathSource;
use crate::utils::PluginKind;

pub enum SetupPluginResult {
  Wasm { plugin_info: PluginInfo },
  Exec { plugin_info: PluginInfo },
  Node { snapshottable: bool, plugin_info: PluginInfo },
}

pub async fn setup_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment),
    Some(PluginKind::Process) => process::setup_process_plugin(url_or_file_path, file_bytes, environment).await,
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

pub fn get_wasm_plugin_file_path<TEnvironment: Environment>(plugin_info: &PluginInfo, environment: &TEnvironment) -> PathBuf {
  wasm::get_file_path_from_plugin_info(plugin_info, environment)
}

pub fn get_exec_plugin_file_path<TEnvironment: Environment>(plugin_info: &PluginInfo, environment: &TEnvironment) -> PathBuf {
  process::get_exec_plugin_file_path(&plugin_info.name, &plugin_info.version, environment)
}

pub fn get_node_process_plugin_file_path<TEnvironment: Environment>(plugin_info: &PluginInfo, environment: &TEnvironment) -> PathBuf {
  process::get_node_plugin_file_path(&plugin_info.name, &plugin_info.version, environment)
}

/// Deletes the plugin from the cache.
pub fn cleanup_plugin<TEnvironment: Environment>(cache_item: &PluginCacheManifestItem, environment: &TEnvironment) -> Result<()> {
  match &cache_item.kind {
    crate::plugins::cache_manifest::PluginCacheManifestItemKind::Wasm => {
      let file_path = wasm::get_file_path_from_plugin_info(&cache_item.info, environment);
      environment.remove_file(file_path)
    }
    crate::plugins::cache_manifest::PluginCacheManifestItemKind::Exec => {
      let dir_path = process::get_exec_plugin_dir_path(&cache_item.info.name, &cache_item.info.version, environment);
      environment.remove_dir_all(dir_path)
    }
    crate::plugins::cache_manifest::PluginCacheManifestItemKind::Node(_) => {
      let dir_path = process::get_node_plugin_dir_path(&cache_item.info.name, &cache_item.info.version, environment);
      environment.remove_dir_all(dir_path)
    }
  }
}

pub async fn create_plugin<TEnvironment: Environment>(
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  plugin_cache: &PluginCache<TEnvironment>,
  environment: TEnvironment,
  plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>> {
  let cache_item = match plugin_cache.get_plugin_cache_item(plugin_reference).await {
    Ok(cache_item) => cache_item,
    Err(err) => {
      log_verbose!(
        environment,
        "Error getting plugin from cache. Forgetting from cache and retrying. Message: {}",
        err.to_string()
      );

      // forget and try again
      plugin_cache.forget_and_recreate(plugin_reference).await?
    }
  };

  match cache_item {
    crate::plugins::PluginCacheItem::Wasm(cache_item) => {
      let file_bytes = match environment.read_file_bytes(&cache_item.compiled_wasm_path) {
        Ok(file_bytes) => file_bytes,
        Err(err) => {
          log_verbose!(
            environment,
            "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {}",
            err.to_string()
          );

          // forget and try again
          let cache_item = plugin_cache.forget_and_recreate(plugin_reference).await?;
          environment.read_file_bytes(cache_item.into_wasm().unwrap().compiled_wasm_path)?
        }
      };

      Ok(Box::new(wasm::WasmPlugin::new(&file_bytes, cache_item.info, environment, plugins_collection)?))
    }
    crate::plugins::PluginCacheItem::Exec(cache_item) => {
      let cache_item = if !environment.path_exists(&cache_item.exe_path) {
        log_verbose!(
          environment,
          "Could not find process plugin at {}. Forgetting from cache and retrying.",
          cache_item.exe_path.display()
        );

        // forget and try again
        plugin_cache.forget_and_recreate(plugin_reference).await?.into_exec().unwrap()
      } else {
        cache_item
      };

      let executable_path = super::process::get_test_safe_executable_path(cache_item.exe_path, &environment);
      Ok(Box::new(process::ProcessPlugin::new(
        environment,
        ProcessPluginExecutableInfo {
          path: executable_path,
          args: Vec::new(),
        },
        cache_item.info,
        plugins_collection,
      )))
    }
    crate::plugins::PluginCacheItem::Node(cache_item) => {
      let node_exe = resolve_node_executable(&environment)?.clone();
      let cache_item = if !environment.path_exists(&cache_item.script) {
        log_verbose!(
          environment,
          "Could not find node plugin at {}. Forgetting from cache and retrying.",
          cache_item.script.display()
        );

        // forget and try again
        plugin_cache.forget_and_recreate(plugin_reference).await?.into_node().unwrap()
      } else {
        cache_item
      };

      Ok(Box::new(process::ProcessPlugin::new(
        environment,
        ProcessPluginExecutableInfo {
          path: node_exe,
          args: vec![cache_item.script.to_string_lossy().into_owned()],
        },
        cache_item.info,
        plugins_collection,
      )))
    }
  }
}
