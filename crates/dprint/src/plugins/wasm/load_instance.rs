use crate::types::ErrBox;
use super::PluginImportObject;

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(compiled_module_bytes: &[u8], import_object: &impl PluginImportObject) -> Result<wasmer_runtime::Instance, ErrBox> {
    let artifact = match wasmer_runtime::cache::Artifact::deserialize(&compiled_module_bytes) {
        Ok(artifact) => artifact,
        Err(err) => { return err!("Error deserializing compiled wasm module: {:?}", err); }
    };
    let compiler = wasmer_runtime::compiler_for_backend(wasmer_runtime::Backend::default()).expect("Expect to have a compiler");
    let module = unsafe { wasmer_runtime_core::load_cache_with(artifact, &*compiler).unwrap() };
    let import_object = import_object.create_import_object();

    match module.instantiate(&import_object) {
        Ok(instance) => Ok(instance),
        Err(err) => err!("Error instantiating module: {}", err),
    }
}
