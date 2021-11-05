use dprint_core::types::ErrBox;
use wasmer::ImportObject;
use wasmer::Instance;
use wasmer::Module;
use wasmer::Store;

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(module: &Module, import_object: &ImportObject) -> Result<Instance, ErrBox> {
  let instance = Instance::new(module, import_object);
  match instance {
    Ok(instance) => Ok(instance),
    Err(err) => err!("Error instantiating module: {}", err),
  }
}

pub fn create_module(compiled_module_bytes: &[u8]) -> Result<Module, ErrBox> {
  let store = Store::default();

  unsafe {
    match Module::deserialize(&store, &compiled_module_bytes) {
      Ok(module) => Ok(module),
      Err(err) => err!("Error deserializing compiled wasm module: {:?}", err),
    }
  }
}
