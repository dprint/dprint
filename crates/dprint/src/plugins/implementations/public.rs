use anyhow::Result;
use anyhow::bail;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::ProcessPluginLaunchInfo;

use super::WasmModuleCreator;
use super::process;
use super::wasm;
use crate::environment::Environment;
use crate::plugins::CachePluginKind;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::utils::PathSource;
use crate::utils::PluginKind;

use super::process::deno::DenoPermissions;
use super::process::deno::default_deno_permissions;
use super::process::deno::resolve_deno_executable;

pub struct SetupPluginResult {
  pub file_path: PathBuf,
  pub plugin_info: PluginInfo,
  pub cache_kind: Option<CachePluginKind>,
  pub permissions: Option<DenoPermissions>,
}

pub async fn setup_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  file_bytes: Vec<u8>,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => wasm::setup_wasm_plugin(url_or_file_path, file_bytes, environment).await,
    Some(PluginKind::Process) => {
      // peek at the kind field to determine if it's a deno or process plugin
      match process::peek_plugin_kind(&file_bytes).as_deref() {
        Some("deno") => process::setup_deno_plugin(url_or_file_path, &file_bytes, environment).await,
        _ => process::setup_process_plugin(url_or_file_path, &file_bytes, environment).await,
      }
    }
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

pub fn get_file_path_from_plugin_info<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_info: &PluginInfo,
  cache_kind: Option<CachePluginKind>,
  environment: &TEnvironment,
) -> Result<PathBuf> {
  if cache_kind == Some(CachePluginKind::Deno) {
    return Ok(process::get_deno_file_path_from_plugin_info(plugin_info, environment));
  }
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => Ok(wasm::get_file_path_from_plugin_info(plugin_info, environment)),
    Some(PluginKind::Process) => Ok(process::get_file_path_from_plugin_info(plugin_info, environment)),
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
  }
}

/// Deletes the plugin from the cache.
pub fn cleanup_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_info: &PluginInfo,
  cache_kind: Option<CachePluginKind>,
  environment: &TEnvironment,
) -> Result<()> {
  if cache_kind == Some(CachePluginKind::Deno) {
    return process::cleanup_deno_plugin(plugin_info, environment);
  }
  match url_or_file_path.plugin_kind() {
    Some(PluginKind::Wasm) => wasm::cleanup_wasm_plugin(plugin_info, environment),
    Some(PluginKind::Process) => process::cleanup_process_plugin(plugin_info, environment),
    None => {
      bail!("Could not resolve plugin type from url or file path: {}", url_or_file_path.display());
    }
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

  match cache_item.kind {
    Some(CachePluginKind::Deno) => {
      let cache_item = if !environment.path_exists(&cache_item.file_path) {
        log_debug!(
          environment,
          "Could not find deno plugin at {}. Forgetting from cache and retrying.",
          cache_item.file_path.display()
        );
        plugin_cache.forget_and_recreate(plugin_reference).await?
      } else {
        cache_item
      };

      // resolve deno executable
      let deno_exe = resolve_deno_executable(&environment)?;

      // use manifest permissions or defaults for launch info;
      // user config permissions are validated later when config is available
      let effective_permissions = cache_item.permissions.clone().unwrap_or_else(default_deno_permissions);
      let plugin_dir = cache_item.file_path.parent().unwrap_or(&cache_item.file_path);

      let launch_info = ProcessPluginLaunchInfo {
        executable: deno_exe,
        pre_args: process::deno::build_deno_pre_args(&effective_permissions, plugin_dir, &cache_item.file_path),
      };
      Ok(Box::new(process::ProcessPlugin::new(environment, launch_info, cache_item.info)))
    }
    _ => {
      // existing wasm/process logic
      match plugin_reference.plugin_kind() {
        Some(PluginKind::Wasm) => {
          let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
              log_debug!(
                environment,
                "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {}",
                err.to_string()
              );

              // forget and try again
              let cache_item = plugin_cache.forget_and_recreate(plugin_reference).await?;
              environment.read_file_bytes(cache_item.file_path)?
            }
          };

          Ok(Box::new(wasm::WasmPlugin::new(&file_bytes, cache_item.info, wasm_module_creator, environment)?))
        }
        Some(PluginKind::Process) => {
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

          let executable_path = super::process::get_test_safe_executable_path(cache_item.file_path, &environment);
          let launch_info = ProcessPluginLaunchInfo::from_executable(executable_path);
          Ok(Box::new(process::ProcessPlugin::new(environment, launch_info, cache_item.info)))
        }
        None => {
          bail!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
        }
      }
    }
  }
}
