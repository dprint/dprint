use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;
use dprint_core::configuration::ConfigKeyMap;
use wasmer::{Function, Store, Memory};
use std::rc::Rc;
use std::cell::RefCell;

use crate::plugins::pool::PluginPools;
use crate::environment::Environment;
use super::super::format_with_plugin_pool;

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
    memory: Option<Memory>,
    override_config: Option<ConfigKeyMap>,
    file_path: Option<PathBuf>,
    shared_bytes: Vec<u8>,
    formatted_text_store: String,
    error_text_store: String,
}

#[derive(Clone)]
pub struct ImportObjectEnvironment<TEnvironment: Environment> {
    parent_plugin_name: String,
    pools: Arc<PluginPools<TEnvironment>>,
    cell: Rc<RefCell<ImportObjectEnvironmentCellItems>>,
}

impl<TEnvironment: Environment> ImportObjectEnvironment<TEnvironment> {
    pub fn new(plugin_name: &str, pools: Arc<PluginPools<TEnvironment>>) -> Self {
        ImportObjectEnvironment {
            parent_plugin_name: plugin_name.to_string(),
            pools,
            cell: Rc::new(RefCell::new(ImportObjectEnvironmentCellItems {
                memory: None,
                override_config: None,
                file_path: None,
                shared_bytes: Vec::new(),
                formatted_text_store: String::new(),
                error_text_store: String::new(),
            }))
        }
    }

    pub fn set_memory(&self, memory: Memory) {
        self.cell.borrow_mut().memory = Some(memory);
    }
}

/// Create an import object that formats text using plugins from the plugin pool
pub fn create_pools_import_object<TEnvironment: Environment>(
    store: &Store,
    import_object_env: &ImportObjectEnvironment<TEnvironment>,
) -> wasmer::ImportObject {
    let host_clear_bytes = {
        |env: &mut ImportObjectEnvironment<TEnvironment>, length: u32| {
            env.cell.borrow_mut().shared_bytes = Vec::with_capacity(length as usize);
        }
    };
    let host_read_buffer = {
        |env: &mut ImportObjectEnvironment<TEnvironment>, buffer_pointer: u32, length: u32| {
            let buffer_pointer: wasmer::WasmPtr<u8, wasmer::Array> = wasmer::WasmPtr::new(buffer_pointer);
            let mut cell = env.cell.borrow_mut();
            // take the memory to prevent mutating while borrowing
            let memory = cell.memory.take().unwrap();
            let memory_reader = buffer_pointer
                .deref(&memory, 0, length)
                .unwrap();
            for i in 0..length as usize {
                cell.shared_bytes.push(memory_reader[i].get());
            }
            // put back the memory
            cell.memory = Some(memory);
        }
    };
    let host_write_buffer = {
        |env: &mut ImportObjectEnvironment<TEnvironment>, buffer_pointer: u32, offset: u32, length: u32| {
            let buffer_pointer: wasmer::WasmPtr<u8, wasmer::Array> = wasmer::WasmPtr::new(buffer_pointer);
            let cell = env.cell.borrow_mut();
            let memory = cell.memory.as_ref().unwrap();
            let memory_writer = buffer_pointer
                .deref(&memory, 0, length)
                .unwrap();
            let offset = offset as usize;
            let length = length as usize;
            let byte_slice = &cell.shared_bytes[offset..offset + length];
            for i in 0..length as usize {
                memory_writer[i].set(byte_slice[i]);
            }
        }
    };
    let host_take_override_config = {
        |env: &mut ImportObjectEnvironment<TEnvironment>| {
            let mut cell = env.cell.borrow_mut();
            let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
            let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or(HashMap::new());
            cell.override_config.replace(config_key_map);
        }
    };
    let host_take_file_path = {
        |env: &mut ImportObjectEnvironment<TEnvironment>| {
            let mut cell = env.cell.borrow_mut();
            let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
            let file_path_str = String::from_utf8(bytes).unwrap();
            cell.file_path.replace(PathBuf::from(file_path_str));
        }
    };
    let host_format = {
        |env: &mut ImportObjectEnvironment<TEnvironment>| {
            let (override_config, file_path, file_text) = {
                let mut cell = env.cell.borrow_mut();
                let override_config = cell.override_config.take().unwrap_or(HashMap::new());
                let file_path = cell.file_path.take().expect("Expected to have file path.");
                let bytes = std::mem::replace(&mut cell.shared_bytes, Vec::new());
                let file_text = String::from_utf8(bytes).unwrap();
                (override_config, file_path, file_text)
            };

            match format_with_plugin_pool(&env.parent_plugin_name, &file_path, &file_text, &override_config, &env.pools) {
                Ok(Some(formatted_text)) => {
                    let mut cell = env.cell.borrow_mut();
                    cell.formatted_text_store = formatted_text;
                    1 // change
                },
                Ok(None) => {
                    0 // no change
                }
                Err(err) => {
                    let mut cell = env.cell.borrow_mut();
                    cell.error_text_store = err.to_string();
                    2 // error
                }
            }
        }
    };
    let host_get_formatted_text = {
        |env: &mut ImportObjectEnvironment<TEnvironment>| {
            let mut cell = env.cell.borrow_mut();
            let formatted_text = std::mem::replace(&mut cell.formatted_text_store, String::new());
            let len = formatted_text.len();
            cell.shared_bytes = formatted_text.into_bytes();
            len as u32
        }
    };
    let host_get_error_text = {
        // todo: reduce code duplication with above function
        |env: &mut ImportObjectEnvironment<TEnvironment>| {
            let mut cell = env.cell.borrow_mut();
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
