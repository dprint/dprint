use dprint_core::types::ErrBox;
use wasmer::{Store, Module};

use crate::plugins::CompilationResult;
use super::{InitializedWasmPlugin, create_identity_import_object};

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
    let store = Store::default();
    let module = Module::new(&store, wasm_bytes)?;
    let bytes = match module.serialize() {
        Ok(bytes) => Ok(bytes),
        Err(err) => err!("Error serializing wasm module: {:?}", err),
    }?;

    // load the plugin and get the info
    let plugin = InitializedWasmPlugin::new(
        module,
        Box::new(move || create_identity_import_object(&store)), // we're not formatting anything so this is ok
    )?;
    let plugin_info = plugin.get_plugin_info()?;

    Ok(CompilationResult {
        bytes,
        plugin_info,
    })
}
