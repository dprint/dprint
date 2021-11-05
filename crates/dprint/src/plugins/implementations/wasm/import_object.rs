use dprint_core::configuration::ConfigKeyMap;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use wasmer::Function;
use wasmer::HostEnvInitError;
use wasmer::Instance;
use wasmer::LazyInit;
use wasmer::Memory;
use wasmer::Store;
use wasmer::WasmerEnv;

use super::super::format_with_plugin_pool;
use crate::environment::Environment;
use crate::plugins::pool::PluginPools;

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
          "host_clear_bytes" => Function::new_native(&store, host_clear_bytes),
          "host_read_buffer" => Function::new_native(&store, host_read_buffer),
          "host_write_buffer" => Function::new_native(&store, host_write_buffer),
          "host_take_override_config" => Function::new_native(&store, host_take_override_config),
          "host_take_file_path" => Function::new_native(&store, host_take_file_path),
          "host_format" => Function::new_native(&store, host_format),
          "host_get_formatted_text" => Function::new_native(&store, host_get_formatted_text),
          "host_get_error_text" => Function::new_native(&store, host_get_error_text),
      }
  }
}

pub struct ImportObjectEnvironmentCellItems {
  override_config: Option<ConfigKeyMap>,
  file_path: Option<PathBuf>,
  shared_bytes: Vec<u8>,
  formatted_text_store: String,
  error_text_store: String,
}

#[derive(Clone)]
pub struct ImportObjectEnvironment<TEnvironment: Environment> {
  parent_plugin_name: String,
  memory: LazyInit<Memory>,
  pools: Arc<PluginPools<TEnvironment>>,
  cell: Arc<Mutex<ImportObjectEnvironmentCellItems>>,
}

impl<TEnvironment: Environment> WasmerEnv for ImportObjectEnvironment<TEnvironment> {
  fn init_with_instance(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
    let memory = instance.exports.get_memory("memory").unwrap();
    self.memory.initialize(memory.clone());
    Ok(())
  }
}

impl<TEnvironment: Environment> ImportObjectEnvironment<TEnvironment> {
  pub fn new(plugin_name: &str, pools: Arc<PluginPools<TEnvironment>>) -> Self {
    ImportObjectEnvironment {
      parent_plugin_name: plugin_name.to_string(),
      pools,
      memory: LazyInit::new(),
      cell: Arc::new(Mutex::new(ImportObjectEnvironmentCellItems {
        override_config: None,
        file_path: None,
        shared_bytes: Vec::new(),
        formatted_text_store: String::new(),
        error_text_store: String::new(),
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
      for i in 0..length as usize {
        cell.shared_bytes.push(memory_reader[i].get());
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
      let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
      let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or(HashMap::new());
      cell.override_config.replace(config_key_map);
    }
  };
  let host_take_file_path = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
      let file_path_str = String::from_utf8(bytes).unwrap();
      cell.file_path.replace(PathBuf::from(file_path_str));
    }
  };
  let host_format = {
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let (override_config, file_path, file_text) = {
        let mut cell = env.cell.lock();
        let override_config = cell.override_config.take().unwrap_or(HashMap::new());
        let file_path = cell.file_path.take().expect("Expected to have file path.");
        let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
        let file_text = String::from_utf8(bytes).unwrap();
        (override_config, file_path, file_text)
      };

      match format_with_plugin_pool(&env.parent_plugin_name, &file_path, &file_text, &override_config, &env.pools) {
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
      let formatted_text = std::mem::replace(&mut cell.formatted_text_store, String::new());
      let len = formatted_text.len();
      cell.shared_bytes = formatted_text.into_bytes();
      len as u32
    }
  };
  let host_get_error_text = {
    // todo: reduce code duplication with above function
    |env: &ImportObjectEnvironment<TEnvironment>| {
      let mut cell = env.cell.lock();
      let error_text = std::mem::replace(&mut cell.error_text_store, String::new());
      let len = error_text.len();
      cell.shared_bytes = error_text.into_bytes();
      len as u32
    }
  };

  wasmer::imports! {
      "dprint" => {
          "host_clear_bytes" => Function::new_native_with_env(&store, import_object_env.clone(), host_clear_bytes),
          "host_read_buffer" => Function::new_native_with_env(&store, import_object_env.clone(), host_read_buffer),
          "host_write_buffer" => Function::new_native_with_env(&store, import_object_env.clone(), host_write_buffer),
          "host_take_override_config" => Function::new_native_with_env(&store, import_object_env.clone(), host_take_override_config),
          "host_take_file_path" => Function::new_native_with_env(&store, import_object_env.clone(), host_take_file_path),
          "host_format" => Function::new_native_with_env(&store, import_object_env.clone(), host_format),
          "host_get_formatted_text" => Function::new_native_with_env(&store, import_object_env.clone(), host_get_formatted_text),
          "host_get_error_text" => Function::new_native_with_env(&store, import_object_env.clone(), host_get_error_text),
      }
  }
}
