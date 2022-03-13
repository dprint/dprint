use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::Host;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use wasmer::Function;
use wasmer::HostEnvInitError;
use wasmer::Instance;
use wasmer::LazyInit;
use wasmer::Memory;
use wasmer::Store;
use wasmer::WasmerEnv;

use crate::environment::Environment;
use crate::plugins::collection::PluginsCollection;

/// Use this when the plugins don't need to format via a plugin pool.
pub fn create_identity_import_object(store: &Store) -> wasmer::ImportObject {
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
      "host_clear_bytes" => Function::new_native(store, host_clear_bytes),
      "host_read_buffer" => Function::new_native(store, host_read_buffer),
      "host_write_buffer" => Function::new_native(store, host_write_buffer),
      "host_take_override_config" => Function::new_native(store, host_take_override_config),
      "host_take_file_path" => Function::new_native(store, host_take_file_path),
      "host_format" => Function::new_native(store, host_format),
      "host_get_formatted_text" => Function::new_native(store, host_get_formatted_text),
      "host_get_error_text" => Function::new_native(store, host_get_error_text),
    }
  }
}

pub struct ImportObjectEnvironmentCellItems<TEnvironment: Environment> {
  override_config: Option<ConfigKeyMap>,
  file_path: Option<PathBuf>,
  shared_bytes: Vec<u8>,
  formatted_text_store: String,
  error_text_store: String,
  environment: TEnvironment,
}

#[derive(Clone)]
pub struct ImportObjectEnvironment<TEnvironment: Environment> {
  memory: LazyInit<Memory>,
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  cell: Arc<Mutex<ImportObjectEnvironmentCellItems<TEnvironment>>>,
}

impl<TEnvironment: Environment> WasmerEnv for ImportObjectEnvironment<TEnvironment> {
  fn init_with_instance(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
    let memory = instance.exports.get_memory("memory").unwrap();
    self.memory.initialize(memory.clone());
    Ok(())
  }
}

impl<TEnvironment: Environment> ImportObjectEnvironment<TEnvironment> {
  pub fn new(environment: TEnvironment, plugins_collection: Arc<PluginsCollection<TEnvironment>>) -> Self {
    ImportObjectEnvironment {
      plugins_collection,
      memory: LazyInit::new(),
      cell: Arc::new(Mutex::new(ImportObjectEnvironmentCellItems {
        override_config: None,
        file_path: None,
        shared_bytes: Vec::new(),
        formatted_text_store: String::new(),
        error_text_store: String::new(),
        environment,
      })),
    }
  }
}

/// Create an import object that formats text using plugins from the plugin pool
pub fn create_pools_import_object<TEnvironment: Environment>(store: &Store, import_object_env: &ImportObjectEnvironment<TEnvironment>) -> wasmer::ImportObject {
  let host_clear_bytes = {
    |env: &ImportObjectEnvironment<TEnvironment>, length: u32| {
      env.cell.lock().shared_bytes = Vec::with_capacity(length as usize);
    }
  };
  let host_read_buffer = {
    |env: &ImportObjectEnvironment<TEnvironment>, buffer_pointer: u32, length: u32| {
      let buffer_pointer: wasmer::WasmPtr<u8, wasmer::Array> = wasmer::WasmPtr::new(buffer_pointer);
      let mut cell = env.cell.lock();
      let memory = env.memory.get_ref().unwrap();
      let memory_reader = buffer_pointer.deref(memory, 0, length).unwrap();
      for byte_cell in memory_reader.iter().take(length as usize) {
        cell.shared_bytes.push(byte_cell.get());
      }
    }
  };
  let host_write_buffer = {
    |env: &ImportObjectEnvironment<TEnvironment>, buffer_pointer: u32, offset: u32, length: u32| {
      let buffer_pointer: wasmer::WasmPtr<u8, wasmer::Array> = wasmer::WasmPtr::new(buffer_pointer);
      let cell = env.cell.lock();
      let memory = env.memory.get_ref().unwrap();
      let memory_writer = buffer_pointer.deref(memory, 0, length).unwrap();
      let offset = offset as usize;
      let length = length as usize;
      let byte_slice = &cell.shared_bytes[offset..offset + length];
      for i in 0..length as usize {
        memory_writer[i].set(byte_slice[i]);
      }
    }
  };
  let host_take_override_config = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let bytes = std::mem::take(&mut cell.shared_bytes);
      let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or_default();
      cell.override_config.replace(config_key_map);
    }
  };
  let host_take_file_path = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let bytes = std::mem::take(&mut cell.shared_bytes);
      let file_path_str = String::from_utf8(bytes).unwrap();
      cell.file_path.replace(PathBuf::from(file_path_str));
    }
  };
  let host_format = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let (override_config, file_path, file_text, runtime_handle) = {
        let mut cell = env.cell.lock();
        let override_config = cell.override_config.take().unwrap_or_default();
        let file_path = cell.file_path.take().expect("Expected to have file path.");
        let bytes = std::mem::take(&mut cell.shared_bytes);
        let file_text = String::from_utf8(bytes).unwrap();
        (override_config, file_path, file_text, cell.environment.runtime_handle())
      };

      let plugins_collection = env.plugins_collection.clone();
      let handle = tokio::task::spawn(async move {
        plugins_collection
          .format(HostFormatRequest {
            file_path,
            file_text,
            range: None,
            override_config,
            // Wasm plugins currently don't support cancellation
            token: Arc::new(NullCancellationToken),
          })
          .await
      });

      match runtime_handle.block_on(handle).unwrap() {
        Ok(Some(formatted_text)) => {
          let mut cell = env.cell.lock();
          cell.formatted_text_store = formatted_text;
          1 // change
        }
        Ok(None) => {
          0 // no change
        }
        Err(err) => {
          let mut cell = env.cell.lock();
          cell.error_text_store = err.to_string();
          2 // error
        }
      }
    }
  };
  let host_get_formatted_text = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let formatted_text = std::mem::take(&mut cell.formatted_text_store);
      let len = formatted_text.len();
      cell.shared_bytes = formatted_text.into_bytes();
      len as u32
    }
  };
  let host_get_error_text = {
    // todo: reduce code duplication with above function
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let error_text = std::mem::take(&mut cell.error_text_store);
      let len = error_text.len();
      cell.shared_bytes = error_text.into_bytes();
      len as u32
    }
  };

  wasmer::imports! {
    "dprint" => {
      "host_clear_bytes" => Function::new_native_with_env(store, import_object_env.clone(), host_clear_bytes),
      "host_read_buffer" => Function::new_native_with_env(store, import_object_env.clone(), host_read_buffer),
      "host_write_buffer" => Function::new_native_with_env(store, import_object_env.clone(), host_write_buffer),
      "host_take_override_config" => Function::new_native_with_env(store, import_object_env.clone(), host_take_override_config),
      "host_take_file_path" => Function::new_native_with_env(store, import_object_env.clone(), host_take_file_path),
      "host_format" => Function::new_native_with_env(store, import_object_env.clone(), host_format),
      "host_get_formatted_text" => Function::new_native_with_env(store, import_object_env.clone(), host_get_formatted_text),
      "host_get_error_text" => Function::new_native_with_env(store, import_object_env.clone(), host_get_error_text),
    }
  }
}
