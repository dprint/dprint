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

// https://github.com/wasmerio/wasmer/pull/3378#issuecomment-1327679422
pub struct WasmModuleCreator {
  engine: wasmer::Engine,
}

impl Default for WasmModuleCreator {
  fn default() -> Self {
    let compiler = Cranelift::default();
    let engine = EngineBuilder::new(compiler).engine();
    let engine: wasmer::Engine = engine.into();
    Self { engine }
  }
}

impl WasmModuleCreator {
  pub fn create_from_wasm_bytes(&self, wasm_bytes: &[u8]) -> Result<Module> {
    let engine_ref = EngineRef::new(&self.engine);
    Ok(Module::new(&engine_ref, wasm_bytes)?)
  }

  pub fn create_from_serialized(&self, compiled_module_bytes: &[u8]) -> Result<Module> {
    unsafe {
      let engine_ref = EngineRef::new(&self.engine);
      match Module::deserialize(&engine_ref, compiled_module_bytes) {
        Ok(module) => Ok(module),
        Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
      }
    }
  }
}
