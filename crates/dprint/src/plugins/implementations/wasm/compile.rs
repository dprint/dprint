use anyhow::Result;

use super::create_identity_import_object;
use super::create_wasm_plugin_instance;
use super::instance::WasmHostState;
use super::load_instance::WasmModuleCreator;
use super::load_instance::load_instance;
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
  let linker = create_identity_import_object(module.version(), module.engine())?;
  let mut store = module.new_store(WasmHostState::Empty);
  let instance = load_instance(&mut store, &module, &linker)?;
  let mut instance = create_wasm_plugin_instance(store, instance)?;

  Ok(CompilationResult {
    bytes,
    plugin_info: instance.plugin_info()?,
  })
}
