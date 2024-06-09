use anyhow::bail;
use anyhow::Result;
use wasmer::Cranelift;
use wasmer::EngineBuilder;
use wasmer::EngineRef;
use wasmer::Imports;
use wasmer::Instance;
use wasmer::Module;
use wasmer::Store;

use super::instance::get_current_plugin_schema_version;
use super::PluginSchemaVersion;

#[derive(Clone)]
pub struct WasmInstance {
  pub inner: wasmer::Instance,
  pub engine: wasmer::Engine,
  version: PluginSchemaVersion,
}

impl WasmInstance {
  pub fn version(&self) -> PluginSchemaVersion {
    self.version
  }
}

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(store: &mut Store, module: &WasmModule, import_object: &Imports) -> Result<WasmInstance> {
  let instance = Instance::new(store, &module.inner, import_object);
  match instance {
    Ok(instance) => Ok(WasmInstance {
      inner: instance,
      engine: module.engine.clone(),
      version: module.version,
    }),
    Err(err) => bail!("Error instantiating module: {:#}", err),
  }
}

#[derive(Clone)]
pub struct WasmModule {
  inner: wasmer::Module,
  engine: wasmer::Engine,
  version: PluginSchemaVersion,
}

impl WasmModule {
  pub fn new(module: wasmer::Module, engine: wasmer::Engine) -> Result<Self> {
    Ok(Self {
      version: get_current_plugin_schema_version(&module)?,
      inner: module,
      engine,
    })
  }

  pub fn version(&self) -> PluginSchemaVersion {
    self.version
  }

  pub fn inner(&self) -> &wasmer::Module {
    &self.inner
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
  pub fn create_from_wasm_bytes(&self, wasm_bytes: &[u8]) -> Result<WasmModule> {
    let engine_ref = EngineRef::new(&self.engine);
    let module = Module::new(&engine_ref, wasm_bytes)?;
    WasmModule::new(module, self.engine.clone())
  }

  pub fn create_from_serialized(&self, compiled_module_bytes: &[u8]) -> Result<WasmModule> {
    unsafe {
      let engine_ref = EngineRef::new(&self.engine);
      match Module::deserialize(&engine_ref, compiled_module_bytes) {
        Ok(module) => WasmModule::new(module, self.engine.clone()),
        Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
      }
    }
  }
}
