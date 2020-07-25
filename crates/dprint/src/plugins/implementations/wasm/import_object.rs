use crate::plugins::pool::PluginPools;
use crate::environment::Environment;
use super::super::format_with_plugin_pool;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::Arc;

/// Use this when the plugins don't need to format via a plugin pool.
pub fn create_identity_import_object() -> wasmer_runtime::ImportObject {
    let host_clear_bytes = |_: u32| {};
    let host_read_buffer = |_: u32, _: u32| {};
    let host_write_buffer = |_: u32, _: u32, _: u32| {};
    let host_take_file_path = || {};
    let host_format = || -> u32 { 0 }; // no change
    let host_get_formatted_text = || -> u32 { 0 }; // zero length
    let host_get_error_text = || -> u32 { 0 }; // zero length

    wasmer_runtime::imports! {
        "dprint" => {
            "host_clear_bytes" => wasmer_runtime::func!(host_clear_bytes),
            "host_read_buffer" => wasmer_runtime::func!(host_read_buffer),
            "host_write_buffer" => wasmer_runtime::func!(host_write_buffer),
            "host_take_file_path" => wasmer_runtime::func!(host_take_file_path),
            "host_format" => wasmer_runtime::func!(host_format),
            "host_get_formatted_text" => wasmer_runtime::func!(host_get_formatted_text),
            "host_get_error_text" => wasmer_runtime::func!(host_get_error_text),
        }
    }
}

/// Create an import object that formats text using plugins from the plugin pool
pub fn create_pools_import_object<TEnvironment: Environment>(
    pools: Arc<PluginPools<TEnvironment>>,
    plugin_name: &str,
) -> wasmer_runtime::ImportObject {
    let parent_plugin_name = String::from(plugin_name);
    let file_path: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let shared_bytes: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::with_capacity(0)));
    let formatted_text_store: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let error_text_store: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let host_clear_bytes = {
        let shared_bytes = shared_bytes.clone();
        move |length: u32| {
            let mut shared_bytes = shared_bytes.lock().unwrap();
            *shared_bytes = Vec::with_capacity(length as usize);
        }
    };
    let host_read_buffer = {
        let shared_bytes = shared_bytes.clone();
        move |ctx: &mut wasmer_runtime::Ctx, buffer_pointer: u32, length: u32| {
            let buffer_pointer: wasmer_runtime::WasmPtr<u8, wasmer_runtime::Array> = wasmer_runtime::WasmPtr::new(buffer_pointer);
            let memory_reader = buffer_pointer
                .deref(ctx.memory(0), 0, length)
                .unwrap();
            let mut shared_bytes = shared_bytes.lock().unwrap();
            for i in 0..length as usize {
                shared_bytes.push(memory_reader[i].get());
            }
        }
    };
    let host_write_buffer = {
        let shared_bytes = shared_bytes.clone();
        move |ctx: &mut wasmer_runtime::Ctx, buffer_pointer: u32, offset: u32, length: u32| {
            let buffer_pointer: wasmer_runtime::WasmPtr<u8, wasmer_runtime::Array> = wasmer_runtime::WasmPtr::new(buffer_pointer);
            let memory_writer = buffer_pointer
                .deref(ctx.memory(0), 0, length)
                .unwrap();
            let offset = offset as usize;
            let length = length as usize;
            let shared_bytes = shared_bytes.lock().unwrap();
            let byte_slice = &shared_bytes[offset..offset + length];
            for i in 0..length as usize {
                memory_writer[i].set(byte_slice[i]);
            }
        }
    };
    let host_take_file_path = {
        let file_path = file_path.clone();
        let shared_bytes = shared_bytes.clone();
        move || {
            let bytes = {
                let mut shared_bytes = shared_bytes.lock().unwrap();
                std::mem::replace(&mut *shared_bytes, Vec::with_capacity(0))
            };
            let file_path_str = String::from_utf8(bytes).unwrap();
            let mut file_path = file_path.lock().unwrap();
            file_path.replace(PathBuf::from(file_path_str));
        }
    };
    let host_format = {
        let file_path = file_path.clone();
        let shared_bytes = shared_bytes.clone();
        let formatted_text_store = formatted_text_store.clone();
        let error_text_store = error_text_store.clone();
        move || {
            let file_path = file_path.lock().unwrap().take().expect("Expected to have file path.");
            let bytes = {
                let mut shared_bytes = shared_bytes.lock().unwrap();
                std::mem::replace(&mut *shared_bytes, Vec::with_capacity(0))
            };
            let file_text = String::from_utf8(bytes).unwrap();

            match format_with_plugin_pool(&parent_plugin_name, &file_path, &file_text, &pools) {
                Ok(Some(formatted_text)) => {
                    let mut formatted_text_store = formatted_text_store.lock().unwrap();
                    *formatted_text_store = formatted_text;
                    1 // change
                },
                Ok(None) => {
                    0 // no change
                }
                Err(err) => {
                    let mut error_text_store = error_text_store.lock().unwrap();
                    *error_text_store = err.to_string();
                    2 // error
                }
            }
        }
    };
    let host_get_formatted_text = {
        let shared_bytes = shared_bytes.clone();
        let formatted_text_store = formatted_text_store.clone();
        move || {
            let formatted_text = {
                let mut formatted_text_store = formatted_text_store.lock().unwrap();
                std::mem::replace(&mut *formatted_text_store, String::new())
            };
            let len = formatted_text.len();
            let mut shared_bytes = shared_bytes.lock().unwrap();
            *shared_bytes = formatted_text.into_bytes();
            len as u32
        }
    };
    let host_get_error_text = {
        // todo: reduce code duplication with above function
        let shared_bytes = shared_bytes.clone();
        let error_text_store = error_text_store.clone();
        move || {
            let error_text = {
                let mut error_text_store = error_text_store.lock().unwrap();
                std::mem::replace(&mut *error_text_store, String::new())
            };
            let len = error_text.len();
            let mut shared_bytes = shared_bytes.lock().unwrap();
            *shared_bytes = error_text.into_bytes();
            len as u32
        }
    };

    wasmer_runtime::imports! {
        "dprint" => {
            "host_clear_bytes" => wasmer_runtime::func!(host_clear_bytes),
            "host_read_buffer" => wasmer_runtime::func!(host_read_buffer),
            "host_write_buffer" => wasmer_runtime::func!(host_write_buffer),
            "host_take_file_path" => wasmer_runtime::func!(host_take_file_path),
            "host_format" => wasmer_runtime::func!(host_format),
            "host_get_formatted_text" => wasmer_runtime::func!(host_get_formatted_text),
            "host_get_error_text" => wasmer_runtime::func!(host_get_error_text),
        }
    }
}
