use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use wasmer::Module;
use wasmer::Store;

use super::create_identity_import_object;
use super::load_instance::load_instance;
use super::InitializedWasmPlugin;
use crate::environment::Environment;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8], environment: impl Environment) -> Result<CompilationResult> {
  let module = Module::new(&Store::default(), wasm_bytes)?;
  let bytes = match module.serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let plugin = InitializedWasmPlugin::new(
    "compiling".to_string(),
    Arc::new(bytes.into()),
    Arc::new(move |store, module| {
      // we're not formatting anything so using an identity import is ok
      let imports = create_identity_import_object(store);
      load_instance(store, &module, &imports)
    }),
    Default::default(),
    Default::default(),
    environment,
  );
  let plugin_info = plugin.get_plugin_info()?;
  let bytes = Arc::try_unwrap(plugin.into_bytes()).unwrap();

  Ok(CompilationResult { bytes, plugin_info })
}
