use anyhow::bail;
use anyhow::Result;
use wasmer::Cranelift;
use wasmer::EngineBuilder;
use wasmer::EngineRef;
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
    let compiler = Cranelift::default();
    let engine = EngineBuilder::new(compiler).engine();
    let engine: wasmer::Engine = engine.into();
    let engine_ref = EngineRef::new(&engine);
    match Module::deserialize(&engine_ref, compiled_module_bytes) {
      Ok(module) => Ok(module),
      Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
    }
  }
}
