use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use wasmer::Module;
use wasmer::Store;

use super::create_identity_import_object;
use super::InitializedWasmPlugin;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let store = Store::default();
  let module = Module::new(&store, wasm_bytes)?;
  let bytes = match module.serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:?}", err),
  };

  // load the plugin and get the info
  let plugin = InitializedWasmPlugin::new(
    module,
    Arc::new(move || create_identity_import_object(&store)), // we're not formatting anything so this is ok
    Default::default(),
    Default::default(),
  );
  let plugin_info = plugin.get_plugin_info()?;

  Ok(CompilationResult { bytes, plugin_info })
}
