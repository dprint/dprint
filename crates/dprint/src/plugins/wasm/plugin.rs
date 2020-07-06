use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration};
use dprint_core::plugins::{PluginInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use bytes::Bytes;

use crate::types::ErrBox;
use super::super::{Plugin, InitializedPlugin};
use super::{ImportObjectFactory, WasmFunctions, FormatResult, load_instance};

pub struct WasmPlugin<TImportObjectFactory : ImportObjectFactory> {
    compiled_wasm_bytes: Bytes,
    plugin_info: PluginInfo,
    config: Option<(HashMap<String, String>, GlobalConfiguration)>,
    import_object_factory: TImportObjectFactory,
}

impl<TImportObjectFactory: ImportObjectFactory> WasmPlugin<TImportObjectFactory> {
    pub fn new(compiled_wasm_bytes: Bytes, plugin_info: PluginInfo, import_object_factory: TImportObjectFactory) -> Self {
        WasmPlugin {
            compiled_wasm_bytes: compiled_wasm_bytes,
            plugin_info,
            config: None,
            import_object_factory,
        }
    }
}

impl<TImportObjectFactory : ImportObjectFactory> Plugin for WasmPlugin<TImportObjectFactory> {
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
        let import_object = self.import_object_factory.create_import_object(self.name());
        let wasm_plugin = InitializedWasmPlugin::new(&self.compiled_wasm_bytes, import_object)?;
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
    pub fn new(compiled_wasm_bytes: &[u8], import_object: wasmer_runtime::ImportObject) -> Result<Self, ErrBox> {
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
