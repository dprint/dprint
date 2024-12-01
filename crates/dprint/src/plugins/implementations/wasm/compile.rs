use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::CancellationToken;
use wasmer::ExportError;
use wasmer::Instance;
use wasmer::Store;

use super::create_wasm_plugin_instance;
use super::instance::create_identity_import_object;
use super::load_instance::load_instance;
use super::load_instance::WasmModuleCreator;
use super::ImportObjectEnvironment;
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

  let bytes = match module.inner().serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let mut store = wasmer::Store::default();
  let imports = create_identity_import_object(module.version(), &mut store);
  let instance = load_instance(&mut store, &module, Box::new(CompileImportObjectEnvironment), &imports)?;
  let mut instance = create_wasm_plugin_instance(store, instance)?;

  Ok(CompilationResult {
    bytes: bytes.into(),
    plugin_info: instance.plugin_info()?,
  })
}
