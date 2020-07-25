use dprint_core::types::ErrBox;

use crate::plugins::CompilationResult;
use super::{InitializedWasmPlugin, create_identity_import_object};

/// Compiles a WASM module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
    let compile_result = wasmer_runtime::compile(&wasm_bytes)?;
    let artifact = compile_result.cache();
    // they didn't implement Error so need to manually handle it here
    let bytes = match artifact {
        Ok(artifact) => match artifact.serialize() {
            Ok(bytes) => Ok(bytes),
            Err(err) => err!("Error serializing wasm module: {:?}", err),
        },
        Err(err) => err!("Error caching wasm module: {:?}", err),
    }?;

    // load the plugin and get the info
    let plugin = InitializedWasmPlugin::new(
        &bytes,
        create_identity_import_object(), // we're not formatting anything so this is ok
    )?;
    let plugin_info = plugin.get_plugin_info()?;

    Ok(CompilationResult {
        bytes,
        plugin_info,
    })
}
