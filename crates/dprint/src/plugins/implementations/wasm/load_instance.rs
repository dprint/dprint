use anyhow::Result;
use anyhow::bail;
use wasmtime::Config;
use wasmtime::Engine;
use wasmtime::Func;
use wasmtime::Memory;
use wasmtime::Module;

use super::PluginSchemaVersion;
use super::instance::Linker;
use super::instance::Store;
use super::instance::WasmHostState;
use super::instance::get_current_plugin_schema_version;

pub struct WasmInstance {
  inner: wasmtime::Instance,
  // note: keep the engine alive for the duration of the instance
  // otherwise it could be cleaned up before the instance is dropped
  _engine: wasmtime::Engine,
  version: PluginSchemaVersion,
}

impl WasmInstance {
  pub fn version(&self) -> PluginSchemaVersion {
    self.version
  }

  pub fn set_token(&self, store: &mut Store, token: std::sync::Arc<dyn dprint_core::plugins::CancellationToken>) {
    store.data_mut().set_token(token);
  }

  pub fn get_memory(&self, store: &mut Store, name: &str) -> Option<Memory> {
    self.inner.get_memory(store, name)
  }

  pub fn get_function(&self, store: &mut Store, name: &str) -> Option<Func> {
    self.inner.get_func(store, name)
  }
}

/// Instantiates a compiled wasm module with the given linker, recording the
/// instance's memory in the store data so host functions can reach it.
pub fn load_instance(store: &mut Store, module: &WasmModule, linker: &Linker) -> Result<WasmInstance> {
  let instance = match linker.instantiate(&mut *store, &module.inner) {
    Ok(instance) => instance,
    Err(err) => bail!("Error instantiating module: {:#}", err),
  };
  if let Some(memory) = instance.get_memory(&mut *store, "memory") {
    store.data_mut().set_memory(memory);
  }
  Ok(WasmInstance {
    inner: instance,
    _engine: module.engine.clone(),
    version: module.version,
  })
}

#[derive(Clone)]
pub struct WasmModule {
  inner: wasmtime::Module,
  engine: wasmtime::Engine,
  version: PluginSchemaVersion,
}

impl WasmModule {
  pub fn new(module: wasmtime::Module, engine: wasmtime::Engine) -> Result<Self> {
    Ok(Self {
      version: get_current_plugin_schema_version(&module)?,
      inner: module,
      engine,
    })
  }

  pub fn version(&self) -> PluginSchemaVersion {
    self.version
  }

  pub fn inner(&self) -> &wasmtime::Module {
    &self.inner
  }

  pub fn engine(&self) -> &wasmtime::Engine {
    &self.engine
  }

  /// Creates a store backed by the same engine as this module so that the
  /// module and the store it's instantiated in always use the same backend.
  pub fn new_store(&self, data: WasmHostState) -> Store {
    Store::new(&self.engine, data)
  }
}

// holds the engine so every module it creates shares one engine, which the
// modules and their stores must agree on
pub struct WasmModuleCreator {
  engine: wasmtime::Engine,
}

impl Default for WasmModuleCreator {
  fn default() -> Self {
    Self { engine: new_engine() }
  }
}

impl WasmModuleCreator {
  pub fn create_from_wasm_bytes(&self, wasm_bytes: &[u8]) -> Result<WasmModule> {
    let module = Module::new(&self.engine, wasm_bytes)?;
    WasmModule::new(module, self.engine.clone())
  }

  /// Creates a module from the serialized native artifact produced by `compile`.
  pub fn create_from_serialized(&self, compiled_module_bytes: &[u8]) -> Result<WasmModule> {
    // SAFETY: the bytes are a cwasm artifact this same binary compiled and wrote
    // to our own cache directory; we never deserialize untrusted input. wasmtime
    // additionally rejects an artifact from an incompatible engine/CPU with an
    // error (the caller then recompiles), though that is a best-effort
    // compatibility check, not a safety boundary against tampered bytes.
    unsafe {
      match Module::deserialize(&self.engine, compiled_module_bytes) {
        Ok(module) => WasmModule::new(module, self.engine.clone()),
        Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
      }
    }
  }
}

/// The amount of wasm stack the plugin may use. wasmtime's default is 512KB,
/// which overflows on deeply nested source files (the formatter recurses over
/// the AST); 1 MiB handles them. The worker thread that runs the instance is
/// given a larger native stack to accommodate this (see `WASM_WORKER_STACK_SIZE`
/// in plugin.rs).
pub const MAX_WASM_STACK_SIZE: usize = 1024 * 1024;

fn new_engine() -> wasmtime::Engine {
  let mut config = Config::new();
  #[cfg(not(use_pulley))]
  {
    // optimize natively compiled plugins for speed
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
  }
  #[cfg(use_pulley)]
  {
    // no native Cranelift backend (or signal-based traps) for this target, so
    // compile to Pulley bytecode and interpret it. every target that sets
    // `use_pulley` is 64-bit, so use the 64-bit Pulley target for the matching
    // endianness.
    let pulley_target = if cfg!(target_endian = "big") { "pulley64be" } else { "pulley64" };
    config.target(pulley_target).expect("failed to set pulley target");
  }
  config.max_wasm_stack(MAX_WASM_STACK_SIZE);
  Engine::new(&config).expect("failed to create wasmtime engine")
}
