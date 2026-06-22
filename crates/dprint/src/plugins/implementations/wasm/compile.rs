use std::sync::Arc;

use anyhow::Result;
#[cfg(not(wasm_interpreter))]
use anyhow::bail;
use dprint_core::plugins::CancellationToken;
use wasmer::ExportError;
use wasmer::Instance;
use wasmer::Store;

use super::ImportObjectEnvironment;
use super::create_wasm_plugin_instance;
use super::instance::create_identity_import_object;
use super::load_instance::WasmModuleCreator;
use super::load_instance::load_instance;
use crate::plugins::CompilationResult;

struct CompileImportObjectEnvironment;

impl ImportObjectEnvironment for CompileImportObjectEnvironment {
  fn initialize(&self, _store: &mut Store, _instance: &Instance) -> Result<(), ExportError> {
    Ok(())
  }
  fn set_token(&self, _store: &mut Store, _token: Arc<dyn CancellationToken>) {}
}

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let wasm_module_creator = WasmModuleCreator::default();
  let module = wasm_module_creator.create_from_wasm_bytes(wasm_bytes)?;

  // the compiler backends cache a serialized native artifact; the interpreter
  // has nothing to compile ahead of time, so it caches the raw wasm bytes and
  // re-parses them on load
  #[cfg(not(wasm_interpreter))]
  let bytes: Vec<u8> = match module.inner().serialize() {
    Ok(bytes) => bytes.into(),
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };
  #[cfg(wasm_interpreter)]
  let bytes: Vec<u8> = wasm_bytes.to_vec();

  // load the plugin and get the info
  let mut store = module.new_store();
  let imports = create_identity_import_object(module.version(), &mut store);
  let instance = load_instance(&mut store, &module, Box::new(CompileImportObjectEnvironment), &imports)?;
  let mut instance = create_wasm_plugin_instance(store, instance)?;

  Ok(CompilationResult {
    bytes,
    plugin_info: instance.plugin_info()?,
  })
}
