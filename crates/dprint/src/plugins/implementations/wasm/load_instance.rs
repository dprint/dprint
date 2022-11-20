use anyhow::bail;
use anyhow::Result;
use wasmer::Imports;
use wasmer::Instance;
use wasmer::Module;
use wasmer::Store;

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(store: &mut Store, module: &Module, import_object: &Imports) -> Result<Instance> {
  let instance = Instance::new(store, module, import_object);
  match instance {
    Ok(instance) => Ok(instance),
    Err(err) => bail!("Error instantiating module: {:#}", err),
  }
}

pub fn create_module(compiled_module_bytes: &[u8]) -> Result<Module> {
  unsafe {
    match Module::deserialize(&Store::default(), compiled_module_bytes) {
      Ok(module) => Ok(module),
      Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
    }
  }
}
