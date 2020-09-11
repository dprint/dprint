use crate::utils::PathSource;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;
use dprint_core::types::ErrBox;

use crate::environment::Environment;

use super::super::SetupPluginResult;

pub fn get_file_path_from_plugin_info(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<PathBuf, ErrBox> {
    let cache_dir_path = environment.get_cache_dir()?;
    let plugin_cache_dir_path = cache_dir_path.join("plugins").join(&plugin_info.name);
    Ok(plugin_cache_dir_path.join(format!("{}-{}.wat", plugin_info.name, plugin_info.version)))
}

pub async fn setup_wasm_plugin<TEnvironment: Environment>(
    url_or_file_path: &PathSource,
    file_bytes: &[u8],
    environment: &TEnvironment
) -> Result<SetupPluginResult, ErrBox> {
    let compile_result = environment.log_action_with_progress(&format!("Compiling {}", url_or_file_path.display()), |_| {
        environment.compile_wasm(file_bytes)
    }, 1).await??;
    let plugin_info = compile_result.plugin_info;
    let plugin_cache_file_path = get_file_path_from_plugin_info(&plugin_info, environment)?;
    environment.mk_dir_all(&plugin_cache_file_path.parent().unwrap().to_path_buf())?;
    environment.write_file_bytes(&plugin_cache_file_path, &compile_result.bytes)?;

    Ok(SetupPluginResult {
        plugin_info,
        file_path: plugin_cache_file_path
    })
}

pub fn cleanup_wasm_plugin(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<(), ErrBox> {
    let plugin_file_path = get_file_path_from_plugin_info(&plugin_info, environment)?;
    environment.remove_file(&plugin_file_path)?;
    Ok(())
}
