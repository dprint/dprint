use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use dprint_core::plugins::CancellationToken;
use wasmer::EngineRef;
use wasmer::ExportError;
use wasmer::Function;
use wasmer::Imports;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::Module;
use wasmer::Store;
#[cfg(not(wasm_interpreter))]
use wasmer::sys::EngineBuilder;

use super::ImportObjectEnvironment;
use super::PluginSchemaVersion;
use super::instance::get_current_plugin_schema_version;

pub struct WasmInstance {
  inner: wasmer::Instance,
  // note: keep the engine alive for the duration of the instance
  // otherwise it could be cleaned up before the instance is dropped
  _engine: wasmer::Engine,
  env: Box<dyn ImportObjectEnvironment>,
  version: PluginSchemaVersion,
}

impl WasmInstance {
  pub fn version(&self) -> PluginSchemaVersion {
    self.version
  }

  pub fn set_token(&self, store: &mut Store, token: Arc<dyn CancellationToken>) {
    self.env.set_token(store, token);
  }

  pub fn get_memory(&self, name: &str) -> Result<&Memory, ExportError> {
    self.inner.exports.get_memory(name)
  }

  pub fn get_function(&self, name: &str) -> Result<&Function, ExportError> {
    self.inner.exports.get_function(name)
  }
}

/// Loads a compiled wasm module from the specified bytes.
pub fn load_instance(store: &mut Store, module: &WasmModule, env: Box<dyn ImportObjectEnvironment>, import_object: &Imports) -> Result<WasmInstance> {
  let instance = Instance::new(store, &module.inner, import_object);
  match instance {
    Ok(instance) => {
      env.initialize(store, &instance)?;
      Ok(WasmInstance {
        inner: instance,
        _engine: module.engine.clone(),
        env,
        version: module.version,
      })
    }
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

  // only the compiler backends serialize the module; the interpreter caches raw
  // wasm instead
  #[cfg(not(wasm_interpreter))]
  pub fn inner(&self) -> &wasmer::Module {
    &self.inner
  }

  /// Creates a store backed by the same engine as this module so that the
  /// module and the store it's instantiated in always use the same backend.
  pub fn new_store(&self) -> Store {
    Store::new(self.engine.clone())
  }
}

// https://github.com/wasmerio/wasmer/pull/3378#issuecomment-1327679422
pub struct WasmModuleCreator {
  engine: wasmer::Engine,
}

impl Default for WasmModuleCreator {
  fn default() -> Self {
    Self { engine: new_engine() }
  }
}

impl WasmModuleCreator {
  pub fn create_from_wasm_bytes(&self, wasm_bytes: &[u8]) -> Result<WasmModule> {
    let engine_ref = EngineRef::new(&self.engine);
    let module = Module::new(&engine_ref, wasm_bytes)?;
    WasmModule::new(module, self.engine.clone())
  }

  /// Creates a module from the bytes cached by `compile`. The compiler backends
  /// cache a serialized native artifact; the interpreter has no artifact to
  /// cache so it stores the raw wasm bytes and parses them here.
  pub fn create_from_serialized(&self, compiled_module_bytes: &[u8]) -> Result<WasmModule> {
    #[cfg(wasm_interpreter)]
    {
      self.create_from_wasm_bytes(compiled_module_bytes)
    }
    #[cfg(not(wasm_interpreter))]
    {
      unsafe {
        let engine_ref = EngineRef::new(&self.engine);
        match Module::deserialize(&engine_ref, compiled_module_bytes) {
          Ok(module) => WasmModule::new(module, self.engine.clone()),
          Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
        }
      }
    }
  }
}

#[cfg(not(wasm_interpreter))]
fn new_engine() -> wasmer::Engine {
  #[cfg(not(target_arch = "loongarch64"))]
  let compiler = wasmer::sys::Cranelift::default();
  #[cfg(target_arch = "loongarch64")]
  let compiler = wasmer::sys::LLVM::default();
  EngineBuilder::new(compiler).engine().into()
}

// wasmi is a portable pure-Rust interpreter used on targets the compiler
// backends can't reach (e.g. powerpc64)
#[cfg(wasm_interpreter)]
fn new_engine() -> wasmer::Engine {
  wasmer::wasmi::engine::Engine::default().into()
}
