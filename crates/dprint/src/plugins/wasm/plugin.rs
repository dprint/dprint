use std::sync::Mutex;
use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration};
use dprint_core::plugins::{PluginInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use bytes::Bytes;
use std::cell::RefCell;
use std::sync::Arc;
use wasmer_runtime_core::{structures::TypedIndex, types::TableIndex};

use crate::types::ErrBox;
use super::super::{Plugin, InitializedPlugin};
use super::{WasmFunctions, FormatResult, load_instance};

pub struct WasmPlugin {
    compiled_wasm_bytes: Bytes,
    plugin_info: PluginInfo,
    config: Option<(HashMap<String, String>, GlobalConfiguration)>,
}

impl WasmPlugin {
    pub fn new(compiled_wasm_bytes: Bytes, plugin_info: PluginInfo) -> WasmPlugin {
        WasmPlugin {
            compiled_wasm_bytes: compiled_wasm_bytes,
            plugin_info,
            config: None,
        }
    }
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.plugin_info.name
    }

    fn version(&self) -> &str {
        &self.plugin_info.version
    }

    fn config_key(&self) -> &str {
        &self.plugin_info.config_key
    }

    fn file_extensions(&self) -> &Vec<String> {
        &self.plugin_info.file_extensions
    }

    fn help_url(&self) -> &str {
        &self.plugin_info.help_url
    }

    fn config_schema_url(&self) -> &str {
        &self.plugin_info.config_schema_url
    }

    fn set_config(&mut self, plugin_config: HashMap<String, String>, global_config: GlobalConfiguration) {
        self.config = Some((plugin_config, global_config));
    }

    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let wasm_plugin = InitializedWasmPlugin::new(&self.compiled_wasm_bytes)?;
        let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config before calling initialize.");

        wasm_plugin.set_global_config(&global_config);
        wasm_plugin.set_plugin_config(&plugin_config);

        Ok(Box::new(wasm_plugin))
    }
}

pub struct InitializedWasmPlugin {
    wasm_functions: WasmFunctions,
    buffer_size: usize,
}

impl InitializedWasmPlugin {
    pub fn new(compiled_wasm_bytes: &[u8]) -> Result<Self, ErrBox> {
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
                let file_path = file_path.lock().unwrap().take();
                let bytes = {
                    let mut shared_bytes = shared_bytes.lock().unwrap();
                    std::mem::replace(&mut *shared_bytes, Vec::with_capacity(0))
                };
                let file_text = String::from_utf8(bytes).unwrap();

                println!("File text: {}", file_text);
                println!("File path: {:?}", file_path);

                let formatted_text = file_text.clone(); // todo: format the file text

                if formatted_text == file_text {
                    0 // no change
                } else {
                    let mut formatted_text_store = formatted_text_store.lock().unwrap();
                    *formatted_text_store = formatted_text;
                    1 // change
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
        let import_object = wasmer_runtime::imports! {
            "dprint" => {
                "host_clear_bytes" => wasmer_runtime::func!(host_clear_bytes),
                "host_read_buffer" => wasmer_runtime::func!(host_read_buffer),
                "host_write_buffer" => wasmer_runtime::func!(host_write_buffer),
                "host_take_file_path" => wasmer_runtime::func!(host_take_file_path),
                "host_format" => wasmer_runtime::func!(host_format),
                "host_get_formatted_text" => wasmer_runtime::func!(host_get_formatted_text),
                "host_get_error_text" => wasmer_runtime::func!(host_get_error_text),
            }
        };
        let instance = load_instance(compiled_wasm_bytes, import_object)?;
        let wasm_functions = WasmFunctions::new(instance)?;
        let buffer_size = wasm_functions.get_wasm_memory_buffer_size();

        Ok(InitializedWasmPlugin {
            wasm_functions,
            buffer_size,
        })
    }

    pub fn set_global_config(&self, global_config: &GlobalConfiguration) {
        let json = serde_json::to_string(global_config).unwrap();
        self.send_string(&json);
        self.wasm_functions.set_global_config();
    }

    pub fn set_plugin_config(&self, plugin_config: &HashMap<String, String>) {
        let json = serde_json::to_string(plugin_config).unwrap();
        self.send_string(&json);
        self.wasm_functions.set_plugin_config();
    }

    pub fn get_plugin_info(&self) -> PluginInfo {
        let len = self.wasm_functions.get_plugin_info();
        let json_text = self.receive_string(len);
        serde_json::from_str(&json_text).unwrap()
    }

    /* LOW LEVEL SENDING AND RECEIVING */

    fn send_string(&self, text: &str) {
        let mut index = 0;
        let len = text.len();
        let text_bytes = text.as_bytes();
        self.wasm_functions.clear_shared_bytes(len);
        while index < len {
            let write_count = std::cmp::min(len - index, self.buffer_size);
            self.write_bytes_to_memory_buffer(&text_bytes[index..(index + write_count)]);
            self.wasm_functions.add_to_shared_bytes_from_buffer(write_count);
            index += write_count;
        }
    }

    fn write_bytes_to_memory_buffer(&self, bytes: &[u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr();
        let memory_writer = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            memory_writer[i].set(bytes[i]);
        }
    }

    fn receive_string(&self, len: usize) -> String {
        let mut index = 0;
        let mut bytes: Vec<u8> = vec![0; len];
        while index < len {
            let read_count = std::cmp::min(len - index, self.buffer_size);
            self.wasm_functions.set_buffer_with_shared_bytes(index, read_count);
            self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)]);
            index += read_count;
        }
        String::from_utf8(bytes).unwrap()
    }

    fn read_bytes_from_memory_buffer(&self, bytes: &mut [u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr();
        let memory_reader = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            bytes[i] = memory_reader[i].get();
        }
    }
}

impl InitializedPlugin for InitializedWasmPlugin {
    fn get_license_text(&self) -> String {
        let len = self.wasm_functions.get_license_text();
        self.receive_string(len)
    }

    fn get_resolved_config(&self) -> String {
        let len = self.wasm_functions.get_resolved_config();
        self.receive_string(len)
    }

    fn get_config_diagnostics(&self) -> Vec<ConfigurationDiagnostic> {
        let len = self.wasm_functions.get_config_diagnostics();
        let json_text = self.receive_string(len);
        serde_json::from_str(&json_text).unwrap()
    }

    fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, String> {
        // send file path
        self.send_string(&file_path.to_string_lossy());
        self.wasm_functions.set_file_path();

        // send file text and format
        self.send_string(file_text);
        let response_code = self.wasm_functions.format();

        // handle the response
        match response_code {
            FormatResult::NoChange => Ok(String::from(file_text)),
            FormatResult::Change => {
                let len = self.wasm_functions.get_formatted_text();
                Ok(self.receive_string(len))
            }
            FormatResult::Error => {
                let len = self.wasm_functions.get_error_text();
                Err(self.receive_string(len))
            }
        }
    }
}
