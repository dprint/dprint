use anyhow::bail;
use anyhow::Result;
use wasmer::ImportObject;
use wasmer::Instance;
use wasmer::Module;
use wasmer::Store;

use super::CompiledWasmModuleBytes;

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(module: &Module, import_object: &ImportObject) -> Result<Instance> {
  let instance = Instance::new(module, import_object);
  match instance {
    Ok(instance) => Ok(instance),
    Err(err) => bail!("Error instantiating module: {:#}", err),
  }
}

pub fn create_module(compiled_module_bytes: &CompiledWasmModuleBytes) -> Result<Module> {
  let store = Store::default();

  unsafe {
    match Module::deserialize(&store, compiled_module_bytes.as_bytes()) {
      Ok(module) => Ok(module),
      Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
    }
  }
}
