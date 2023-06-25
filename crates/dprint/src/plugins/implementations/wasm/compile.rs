use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;

use super::create_identity_import_object;
use super::load_instance::load_instance;
use super::load_instance::WasmModuleCreator;
use super::InitializedWasmPlugin;
use crate::environment::Environment;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8], environment: impl Environment) -> Result<CompilationResult> {
  // https://github.com/wasmerio/wasmer/pull/3378#issuecomment-1327679422
  let wasm_module_creator = WasmModuleCreator::default();
  let module = wasm_module_creator.create_from_wasm_bytes(&wasm_bytes)?;

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
      let imports = create_identity_import_object(store);
      load_instance(store, &module, &imports)
    }),
    Default::default(),
    Default::default(),
    environment,
  );

  Ok(CompilationResult {
    bytes: bytes.into(),
    plugin_info: plugin.get_plugin_info()?,
  })
}
