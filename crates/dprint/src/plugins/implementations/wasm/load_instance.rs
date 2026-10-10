use std::path::Path;

use anyhow::Result;
use anyhow::bail;
use deno_semver::Version;
use dprint_core::plugins::PluginInfo;
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
use super::instance::wasi::add_unsupported_wasi_imports;

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
/// instance's memory in the store data so host functions can reach it, then
/// runs the module's initializer when it has one that it doesn't run itself.
pub fn load_instance(store: &mut Store, module: &WasmModule, mut linker: Linker) -> Result<WasmInstance> {
  // a WASI "command" only initializes its libc when running its main function,
  // which leaves nothing initialized for the plugin's other exports
  if module.inner.get_export("_start").is_some() && !module.has_initialize_export() {
    bail!(
      "Error instantiating module: The plugin was built as a WASI command (it exports _start), but it must be built as a WASI reactor (exporting _initialize)."
    );
  }
  if let Err(err) = add_unsupported_wasi_imports(&mut linker, store, &module.inner) {
    bail!("Error instantiating module: {:#}", err);
  }
  let instance = match linker.instantiate(&mut *store, &module.inner) {
    Ok(instance) => instance,
    Err(err) => bail!("Error instantiating module: {:#}", err),
  };
  if let Some(memory) = instance.get_memory(&mut *store, "memory") {
    store.data_mut().set_memory(memory);
  }
  // plugins linked against a WASI libc are "reactors", which need to be
  // initialized before any of their other exports are called
  if !module.initializes_on_start
    && let Some(initialize) = instance.get_func(&mut *store, "_initialize")
    && let Err(err) = initialize.call(&mut *store, &[], &mut [])
  {
    bail!("Error initializing module: {:#}", err);
  }
  Ok(WasmInstance {
    inner: instance,
    _engine: module.engine.clone(),
    version: module.version,
  })
}

/// Gets if the plugin is known to run `_initialize` as its wasm start function
/// while still exporting it, in which case the host must not call it again.
///
/// This is hardcoded for old versions of dprint-plugin-gofumpt, which trap when
/// initialized a second time (https://github.com/dprint/dprint/issues/1306).
pub fn plugin_initializes_on_start(plugin_info: &PluginInfo) -> bool {
  plugin_info.name == "dprint-plugin-gofumpt"
    && Version::parse_from_npm(&plugin_info.version).is_ok_and(|version| version < Version::parse_from_npm("0.0.19").unwrap())
}

#[derive(Clone)]
pub struct WasmModule {
  inner: wasmtime::Module,
  engine: wasmtime::Engine,
  version: PluginSchemaVersion,
  initializes_on_start: bool,
}

impl WasmModule {
  pub fn new(module: wasmtime::Module, engine: wasmtime::Engine) -> Result<Self> {
    Ok(Self {
      version: get_current_plugin_schema_version(&module)?,
      inner: module,
      engine,
      initializes_on_start: false,
    })
  }

  /// Marks the module as running `_initialize` itself when instantiated,
  /// so that loading an instance doesn't call it a second time.
  pub fn with_initializes_on_start(mut self, value: bool) -> Self {
    self.initializes_on_start = value;
    self
  }

  pub fn has_initialize_export(&self) -> bool {
    self.inner.get_export("_initialize").is_some()
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

/// A hash of everything wasmtime checks before loading a precompiled module:
/// the target, the cpu features the code was tuned for, the compiler settings
/// and the wasmtime version. Including it in the plugin cache key gives artifacts compiled on
/// different cpus distinct cache entries, so they can coexist in a cache
/// directory shared across machines (ex. restored from a CI cache) instead of
/// overwriting each other on every machine change.
pub fn precompile_compatibility_hash() -> u64 {
  static HASH: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
  *HASH.get_or_init(|| {
    use std::hash::Hash;
    let mut hasher = crate::utils::FastInsecureHasher::default();
    new_engine().precompile_compatibility_hash().hash(&mut hasher);
    hasher.finish()
  })
}

// holds the engine so every module it creates shares one engine, which the
// modules and their stores must agree on
#[derive(Clone)]
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

  /// Creates a module by memory mapping the serialized native artifact
  /// produced by `compile`, which loads its pages lazily instead of reading
  /// and copying the whole file up front.
  pub fn create_from_serialized_file(&self, file_path: &Path) -> Result<WasmModule> {
    // SAFETY: same as `create_from_serialized`. Additionally, the file's
    // contents must not change while the module is alive. The cache only ever
    // replaces an artifact by renaming a new file over it or deletes it. On
    // unix both leave an existing mapping of the old file intact and Windows
    // doesn't allow either while the file is mapped (wasmtime keeps the file
    // open without sharing write or delete access).
    unsafe {
      match Module::deserialize_file(&self.engine, file_path) {
        Ok(module) => WasmModule::new(module, self.engine.clone()),
        Err(err) => bail!("Error deserializing compiled wasm module: {:#}", err),
      }
    }
  }
}

/// The amount of wasm stack the plugin may use. wasmtime's default is 512KB,
/// which overflows on deeply nested source files (the formatter recurses over
/// the AST); 1 MiB handles them. The thread that runs the instance needs a
/// native stack at least this large (see `WASM_PLUGIN_THREAD_STACK_SIZE`).
pub const MAX_WASM_STACK_SIZE: usize = 1024 * 1024;

/// Native stack size for the (tokio blocking) thread that runs a wasm plugin
/// instance. wasmtime executes wasm on this native stack, so it must exceed
/// `MAX_WASM_STACK_SIZE` (the hard cap on wasm stack usage) with headroom for the
/// host frames around the wasm call — which is a shallow path, so 3 MiB is ample.
/// Applied via the tokio runtime's `thread_stack_size` since the instance loop
/// runs on `spawn_blocking` (tokio's default blocking-thread stack is too small
/// on some platforms, e.g. Windows). The stack is reserved address space that the
/// OS commits lazily as it's touched, so the unused headroom costs no physical
/// memory; it just needs to be large enough that wasm hits its own limit (a
/// recoverable trap) before exhausting the native stack (a crash).
pub const WASM_PLUGIN_THREAD_STACK_SIZE: usize = MAX_WASM_STACK_SIZE + 3 * 1024 * 1024;

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

#[cfg(test)]
mod tests {
  use wasmtime::Val;

  use super::super::instance::create_identity_import_object;
  use super::*;

  #[test]
  fn initializes_wasi_reactor() {
    let (mut store, instance) = load(
      r#"(import "wasi_snapshot_preview1" "sock_shutdown" (func (param i32 i32) (result i32)))
         (global $count (mut i32) (i32.const 0))
         (func (export "_initialize") (global.set $count (i32.add (global.get $count) (i32.const 1))))
         (func (export "get_count") (result i32) (global.get $count))"#,
    )
    .unwrap();
    let mut results = [Val::I32(0)];
    let get_count = instance.get_function(&mut store, "get_count").unwrap();
    get_count.call(&mut store, &[], &mut results).unwrap();
    assert_eq!(results[0].unwrap_i32(), 1);
  }

  #[test]
  fn skips_initialize_when_module_initializes_on_start() {
    // traps when initialized a second time
    let body = r#"(global $count (mut i32) (i32.const 0))
         (func $init (export "_initialize")
           (if (global.get $count) (then unreachable))
           (global.set $count (i32.const 1)))
         (start $init)"#;
    let err = load(body).err().unwrap();
    assert!(format!("{:#}", err).starts_with("Error initializing module: "), "{:#}", err);
    assert!(load_with(body, |module| module.with_initializes_on_start(true)).is_ok());
  }

  #[test]
  fn gofumpt_initializes_on_start() {
    let run = |name: &str, version: &str| {
      plugin_initializes_on_start(&PluginInfo {
        name: name.to_string(),
        version: version.to_string(),
        config_key: String::new(),
        help_url: String::new(),
        config_schema_url: String::new(),
        update_url: None,
      })
    };
    assert!(run("dprint-plugin-gofumpt", "0.0.1"));
    assert!(run("dprint-plugin-gofumpt", "0.0.18"));
    assert!(!run("dprint-plugin-gofumpt", "0.0.19"));
    assert!(!run("dprint-plugin-gofumpt", "0.1.0"));
    assert!(!run("dprint-plugin-gofumpt", "invalid"));
    assert!(!run("dprint-plugin-other", "0.0.18"));
  }

  #[test]
  fn errors_when_initializing_fails() {
    let err = load(
      r#"(import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
         (func (export "_initialize") (call $proc_exit (i32.const 1)))"#,
    )
    .err()
    .unwrap();
    let text = format!("{:#}", err);
    assert!(text.starts_with("Error initializing module: "), "{}", text);
    assert!(text.contains("The plugin attempted to exit with code 1."), "{}", text);
  }

  #[test]
  fn errors_for_wasi_command() {
    let err = load(r#"(func (export "_start"))"#).err().unwrap();
    assert_eq!(
      format!("{:#}", err),
      "Error instantiating module: The plugin was built as a WASI command (it exports _start), but it must be built as a WASI reactor (exporting _initialize)."
    );
    // ok when it has both
    assert!(load(r#"(func (export "_start")) (func (export "_initialize"))"#).is_ok());
  }

  #[test]
  fn errors_for_unknown_import() {
    let err = load(r#"(import "dprint" "host_unknown" (func))"#).err().unwrap();
    let text = format!("{:#}", err);
    assert!(text.starts_with("Error instantiating module: "), "{}", text);
    assert!(text.contains("host_unknown"), "{}", text);
  }

  fn load(body: &str) -> Result<(Store, WasmInstance)> {
    load_with(body, |module| module)
  }

  fn load_with(body: &str, map_module: impl FnOnce(WasmModule) -> WasmModule) -> Result<(Store, WasmInstance)> {
    let wasm = wat::parse_str(format!(
      r#"(module {} (func (export "dprint_plugin_version_4") (result i32) (i32.const 4)))"#,
      body
    ))?;
    let module = map_module(WasmModuleCreator::default().create_from_wasm_bytes(&wasm)?);
    let linker = create_identity_import_object(module.version(), module.engine())?;
    let mut store = module.new_store(WasmHostState::Empty);
    let instance = load_instance(&mut store, &module, linker)?;
    Ok((store, instance))
  }
}
