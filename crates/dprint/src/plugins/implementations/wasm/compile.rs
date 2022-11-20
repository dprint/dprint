use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use wasmer::Module;
use wasmer::Store;

use super::create_identity_import_object;
use super::load_instance::load_instance;
use super::InitializedWasmPlugin;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let module = Module::new(&Store::default(), wasm_bytes)?;
  let bytes = match module.serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let plugin = InitializedWasmPlugin::new(
    "compiling".to_string(),
    module,
    Arc::new(move |store, module| {
      // we're not formatting anything so using an identity import is ok
      let mut store = store.lock();
      let imports = create_identity_import_object(&mut store);
      //let module = Module::new(&mut *store, &*wasm_bytes)?;
      load_instance(&mut store, &module, &imports)
    }),
    Default::default(),
    Default::default(),
  );
  let plugin_info = plugin.get_plugin_info()?;

  Ok(CompilationResult {
    bytes: bytes.into(),
    plugin_info,
  })
}
