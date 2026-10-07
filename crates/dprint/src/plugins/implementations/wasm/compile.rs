use anyhow::Result;
use dprint_core::plugins::PluginInfo;

use super::create_identity_import_object;
use super::create_wasm_plugin_instance;
use super::instance::WasmHostState;
use super::load_instance::WasmModule;
use super::load_instance::WasmModuleCreator;
use super::load_instance::load_instance;
use super::load_instance::plugin_initializes_on_start;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let wasm_module_creator = WasmModuleCreator::default();
  let module = wasm_module_creator.create_from_wasm_bytes(wasm_bytes)?;

  // cache the serialized native artifact so it can be loaded without recompiling
  let bytes: Vec<u8> = match module.inner().serialize() {
    Ok(bytes) => bytes,
    Err(err) => anyhow::bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let plugin_info = match get_plugin_info(&module) {
    Ok(plugin_info) => plugin_info,
    Err(err) => {
      // the plugin's name and version is only known once it's loaded, so on failure
      // check if this is a plugin that's known to initialize itself on start
      let module = module.clone().with_initializes_on_start(true);
      match module.has_initialize_export().then(|| get_plugin_info(&module)) {
        Some(Ok(plugin_info)) if plugin_initializes_on_start(&plugin_info) => plugin_info,
        _ => return Err(err),
      }
    }
  };

  Ok(CompilationResult { bytes, plugin_info })
}

fn get_plugin_info(module: &WasmModule) -> Result<PluginInfo> {
  let linker = create_identity_import_object(module.version(), module.engine())?;
  let mut store = module.new_store(WasmHostState::Empty);
  let instance = load_instance(&mut store, module, linker)?;
  create_wasm_plugin_instance(store, instance)?.plugin_info()
}
