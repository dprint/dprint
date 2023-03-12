use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use wasmer::Module;
use wasmer::Store;

use super::create_identity_import_object;
use super::InitializedWasmPlugin;
use crate::environment::Environment;
use crate::plugins::CompilationResult;

/// Compiles a Wasm module.
pub fn compile(wasm_bytes: &[u8]) -> Result<CompilationResult> {
  let store = Store::default();
  let module = Module::new(&store, wasm_bytes)?;
  let bytes = match module.serialize() {
    Ok(bytes) => bytes,
    Err(err) => bail!("Error serializing wasm module: {:#}", err),
  };

  // load the plugin and get the info
  let plugin = InitializedWasmPlugin::new(
    "compiling".to_string(),
    module,
    Arc::new(move || create_identity_import_object(&store)), // we're not formatting anything so this is ok
    Default::default(),
    Default::default(),
  );
  let plugin_info = plugin.get_plugin_info()?;

  Ok(CompilationResult { bytes, plugin_info })
}

fn get_magic_header() -> String {
  // todo: make this a constant once rust supports it
  let compiler_version = env!("WASMER_COMPILER_VERSION");
  format!("dprint{}{}", compiler_version.len(), compiler_version)
}

pub fn write_compiled_to_file(environment: &impl Environment, file_path: &Path, wasm_bytes: &[u8]) -> Result<()> {
  if environment.is_real() {
    // optimization to avoid allocating potentially a lot of additional memory
    let mut file = std::fs::File::create(file_path)?;
    file.write_all(get_magic_header().as_bytes())?;
    file.write_all(wasm_bytes)?;
  } else {
    // NOTICE: take extra care to ensure the above is the same
    let magic_header = get_magic_header();
    let mut bytes = Vec::with_capacity(magic_header.len() + wasm_bytes.len());
    bytes.extend(magic_header.as_bytes());
    bytes.extend(wasm_bytes);
    environment.write_file_bytes(file_path, &bytes)?;
  }
  Ok(())
}

pub struct CompiledWasmModuleBytes(Vec<u8>);

impl CompiledWasmModuleBytes {
  pub fn as_bytes(&self) -> &[u8] {
    &self.0
  }
}

pub fn read_compiled_from_file(environment: &impl Environment, file_path: &Path) -> Result<CompiledWasmModuleBytes> {
  let mut file_bytes = environment.read_file_bytes(file_path)?;
  let magic_header = get_magic_header();
  if !file_bytes.starts_with(magic_header.as_bytes()) {
    bail!("Not a valid cached serialized wasm file.");
  }
  // todo: when upgrading wasmer to 3.x this drain can be removed to instead slice
  // on the vector above. Unfortunately in wasmer 2.x not draining from the vector
  // will cause an error when deserializing for some strange reason
  file_bytes.drain(..magic_header.len());
  Ok(CompiledWasmModuleBytes(file_bytes))
}
