use anyhow::bail;
use anyhow::Result;

use super::create_identity_import_object;
use super::create_wasm_plugin_instance;
use super::load_instance::load_instance;
use super::load_instance::WasmModuleCreator;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let wasm_module_creator = WasmModuleCreator::default();
  let module = wasm_module_creator.create_from_wasm_bytes(wasm_bytes)?;

  let bytes = match module.inner().serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let mut store = wasmer::Store::default();
  let imports = create_identity_import_object(&mut store);
  let instance = load_instance(&mut store, &module, &imports)?;
  let mut instance = create_wasm_plugin_instance(store, instance)?;

  Ok(CompilationResult {
    bytes: bytes.into(),
    plugin_info: instance.plugin_info()?,
  })
}
