use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use wasmer::AsStoreRef;
use wasmer::ExportError;
use wasmer::Function;
use wasmer::FunctionEnv;
use wasmer::FunctionEnvMut;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::Store;

use crate::environment::Environment;

/// Use this when the plugins don't need to format via a plugin pool.
pub fn create_identity_import_object(store: &mut Store) -> wasmer::Imports {
  let host_clear_bytes = |_: u32| {};
  let host_read_buffer = |_: u32, _: u32| {};
  let host_write_buffer = |_: u32, _: u32, _: u32| {};
  let host_take_override_config = || {};
  let host_take_file_path = || {};
  let host_format = || -> u32 { 0 }; // no change
  let host_get_formatted_text = || -> u32 { 0 }; // zero length
  let host_get_error_text = || -> u32 { 0 }; // zero length

  wasmer::imports! {
    "dprint" => {
      "host_clear_bytes" => Function::new_typed(store, host_clear_bytes),
      "host_read_buffer" => Function::new_typed(store, host_read_buffer),
      "host_write_buffer" => Function::new_typed(store, host_write_buffer),
      "host_take_override_config" => Function::new_typed(store, host_take_override_config),
      "host_take_file_path" => Function::new_typed(store, host_take_file_path),
      "host_format" => Function::new_typed(store, host_format),
      "host_get_formatted_text" => Function::new_typed(store, host_get_formatted_text),
      "host_get_error_text" => Function::new_typed(store, host_get_error_text),
    }
  }
}

#[derive(Clone)]
pub struct WasmHostFormatCell(Rc<RefCell<Option<HostFormatCallback>>>);

impl WasmHostFormatCell {
  pub fn no_op() -> Self {
    Self(Default::default())
  }

  pub fn new() -> Self {
    Self::no_op()
  }

  pub fn set(&self, host_format: Option<HostFormatCallback>) {
    *self.0.borrow_mut() = host_format;
  }

  pub fn get(&self) -> Option<HostFormatCallback> {
    self.0.borrow().clone()
  }

  pub fn clear(&self) {
    *self.0.borrow_mut() = None;
  }
}

pub type WasmHostFormatCallback = Box<dyn Fn(HostFormatRequest) -> FormatResult + Send>;

/// Create an import object that formats text using plugins from the plugin pool
pub fn create_pools_import_object<TEnvironment: Environment>(
  environment: TEnvironment,
  store: &mut Store,
  host_format_callback: WasmHostFormatCallback,
) -> (wasmer::Imports, FunctionEnv<ImportObjectEnvironment<TEnvironment>>) {
  let env = ImportObjectEnvironment::new(environment, host_format_callback);
  let env = FunctionEnv::new(store, env);

  (
    wasmer::imports! {
      "dprint" => {
        "host_clear_bytes" => Function::new_typed_with_env(store, &env, host_clear_bytes),
        "host_read_buffer" => Function::new_typed_with_env(store, &env, host_read_buffer),
        "host_write_buffer" => Function::new_typed_with_env(store, &env, host_write_buffer),
        "host_take_override_config" => Function::new_typed_with_env(store, &env, host_take_override_config),
        "host_take_file_path" => Function::new_typed_with_env(store, &env, host_take_file_path),
        "host_format" => Function::new_typed_with_env(store, &env, host_format),
        "host_get_formatted_text" => Function::new_typed_with_env(store, &env, host_get_formatted_text),
        "host_get_error_text" => Function::new_typed_with_env(store, &env, host_get_error_text),
      }
    },
    env,
  )
}

#[derive(Default)]
struct SharedBytes {
  data: Vec<u8>,
  index: usize,
}

impl SharedBytes {
  pub fn with_size(size: usize) -> Self {
    Self::from_bytes(vec![0; size])
  }

  pub fn from_bytes(data: Vec<u8>) -> Self {
    Self { data, index: 0 }
  }
}

pub struct ImportObjectEnvironment<TEnvironment: Environment> {
  memory: Option<Memory>,
  override_config: Option<ConfigKeyMap>,
  file_path: Option<PathBuf>,
  formatted_text_store: String,
  shared_bytes: Mutex<SharedBytes>,
  error_text_store: String,
  environment: TEnvironment,
  host_format_callback: WasmHostFormatCallback,
}

impl<TEnvironment: Environment> ImportObjectEnvironment<TEnvironment> {
  pub fn new(environment: TEnvironment, host_format_callback: WasmHostFormatCallback) -> Self {
    ImportObjectEnvironment {
      memory: None,
      override_config: None,
      file_path: None,
      shared_bytes: Mutex::new(SharedBytes::default()),
      formatted_text_store: String::new(),
      error_text_store: String::new(),
      environment,
      host_format_callback,
    }
  }

  pub fn initialize(&mut self, instance: &Instance) -> Result<(), ExportError> {
    self.memory = Some(instance.exports.get_memory("memory")?.clone());
    Ok(())
  }

  fn take_shared_bytes(&self) -> Vec<u8> {
    let mut shared_bytes = self.shared_bytes.lock();
    let data = std::mem::take(&mut shared_bytes.data);
    shared_bytes.index = 0;
    data
  }
}

fn host_clear_bytes<TEnvironment: Environment>(env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>, length: u32) {
  let env = env.data();
  *env.shared_bytes.lock() = SharedBytes::with_size(length as usize);
}

fn host_read_buffer<TEnvironment: Environment>(env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>, buffer_pointer: u32, length: u32) {
  let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
  let env_data = env.data();
  let memory = env_data.memory.as_ref().unwrap();
  let store_ref = env.as_store_ref();
  let memory_view = memory.view(&store_ref);

  let length = length as usize;
  let mut shared_bytes = env_data.shared_bytes.lock();
  let shared_bytes_index = shared_bytes.index;
  memory_view
    .read(
      buffer_pointer.offset() as u64,
      &mut shared_bytes.data[shared_bytes_index..shared_bytes_index + length],
    )
    .unwrap();
  shared_bytes.index += length;
}

fn host_write_buffer<TEnvironment: Environment>(env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>, buffer_pointer: u32, offset: u32, length: u32) {
  let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
  let env_data = env.data();
  let memory = env_data.memory.as_ref().unwrap();
  let store_ref = env.as_store_ref();
  let memory_view = memory.view(&store_ref);
  let offset = offset as usize;
  let length = length as usize;
  let shared_bytes = env_data.shared_bytes.lock();
  memory_view
    .write(buffer_pointer.offset() as u64, &shared_bytes.data[offset..offset + length])
    .unwrap();
}

fn host_take_override_config<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>) {
  let env = env.data_mut();
  let bytes = env.take_shared_bytes();
  let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or_default();
  env.override_config.replace(config_key_map);
}

fn host_take_file_path<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>) {
  let env = env.data_mut();
  let bytes = env.take_shared_bytes();
  let file_path_str = String::from_utf8(bytes).unwrap();
  env.file_path.replace(PathBuf::from(file_path_str));
}

fn host_format<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>) -> u32 {
  let env = env.data_mut();
  let override_config = env.override_config.take().unwrap_or_default();
  let file_path = env.file_path.take().expect("Expected to have file path.");
  let bytes = env.take_shared_bytes();
  let file_text = String::from_utf8(bytes).unwrap();
  let request = HostFormatRequest {
    file_path,
    file_text,
    range: None,
    override_config,
    // Wasm plugins currently don't support cancellation
    token: Arc::new(NullCancellationToken),
  };
  // todo: should the env be released during this call? I think no
  let result = (env.host_format_callback)(request);

  match result {
    Ok(Some(formatted_text)) => {
      //let mut env = env.data_mut();
      env.formatted_text_store = formatted_text;
      1 // change
    }
    Ok(None) => {
      0 // no change
    }
    // ignore critical error as we can just continue formatting
    Err(err) => {
      //let mut env = env.data_mut();
      env.error_text_store = err.to_string();
      2 // error
    }
  }
}

fn host_get_formatted_text<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>) -> u32 {
  let env = env.data_mut();
  let formatted_text = std::mem::take(&mut env.formatted_text_store);
  let len = formatted_text.len();
  *env.shared_bytes.lock() = SharedBytes::from_bytes(formatted_text.into_bytes());
  len as u32
}

fn host_get_error_text<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironment<TEnvironment>>) -> u32 {
  let env = env.data_mut();
  let error_text = std::mem::take(&mut env.error_text_store);
  let len = error_text.len();
  *env.shared_bytes.lock() = SharedBytes::from_bytes(error_text.into_bytes());
  len as u32
}
