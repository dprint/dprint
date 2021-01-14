use std::path::Path;
use std::sync::Arc;

use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration, ConfigKeyMap};
use dprint_core::plugins::{PluginInfo};
use dprint_core::types::ErrBox;

use crate::environment::Environment;
use crate::plugins::{Plugin, InitializedPlugin, PluginPools};
use super::{WasmFunctions, FormatResult, load_instance, create_module, create_pools_import_object, ImportObjectEnvironment};

pub struct WasmPlugin<TEnvironment: Environment> {
    module: wasmer::Module,
    plugin_info: PluginInfo,
    config: Option<(ConfigKeyMap, GlobalConfiguration)>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> WasmPlugin<TEnvironment> {
    pub fn new(compiled_wasm_bytes: Vec<u8>, plugin_info: PluginInfo, plugin_pools: Arc<PluginPools<TEnvironment>>) -> Result<Self, ErrBox> {
        let module = create_module(&compiled_wasm_bytes)?;
        Ok(WasmPlugin {
            module,
            plugin_info,
            config: None,
            plugin_pools,
        })
    }
}

impl<TEnvironment: Environment> Plugin for WasmPlugin<TEnvironment> {
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

    fn set_config(&mut self, plugin_config: ConfigKeyMap, global_config: GlobalConfiguration) {
        self.config = Some((plugin_config, global_config));
    }

    fn get_config(&self) -> &(ConfigKeyMap, GlobalConfiguration) {
        self.config.as_ref().expect("Call set_config first.")
    }

    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let store = wasmer::Store::default();
        let import_obj_env = ImportObjectEnvironment::new(self.name(), self.plugin_pools.clone());
        let import_object = create_pools_import_object(&store, &import_obj_env);
        let wasm_plugin = InitializedWasmPlugin::new(&self.module, &import_object)?;
        let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config first.");

        wasm_plugin.set_global_config(&global_config)?;
        wasm_plugin.set_plugin_config(&plugin_config)?;

        Ok(Box::new(wasm_plugin))
    }
}

pub struct InitializedWasmPlugin {
    wasm_functions: WasmFunctions,
    buffer_size: usize,
}

impl InitializedWasmPlugin {
    pub fn new(module: &wasmer::Module, import_object: &wasmer::ImportObject) -> Result<Self, ErrBox> {
        let instance = load_instance(module, import_object)?;
        let wasm_functions = WasmFunctions::new(instance)?;
        let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;

        Ok(InitializedWasmPlugin {
            wasm_functions,
            buffer_size,
        })
    }

    pub fn set_global_config(&self, global_config: &GlobalConfiguration) -> Result<(), ErrBox> {
        let json = serde_json::to_string(global_config)?;
        self.send_string(&json);
        self.wasm_functions.set_global_config()?;
        Ok(())
    }

    pub fn set_plugin_config(&self, plugin_config: &ConfigKeyMap) -> Result<(), ErrBox> {
        let json = serde_json::to_string(plugin_config)?;
        self.send_string(&json);
        self.wasm_functions.set_plugin_config()?;
        Ok(())
    }

    pub fn get_plugin_info(&self) -> Result<PluginInfo, ErrBox> {
        let len = self.wasm_functions.get_plugin_info()?;
        let json_text = self.receive_string(len)?;
        Ok(serde_json::from_str(&json_text)?)
    }

    /* LOW LEVEL SENDING AND RECEIVING */

    // These methods should panic when failing because that may indicate
    // a major problem where the CLI is out of sync with the plugin.

    fn send_string(&self, text: &str) {
        let mut index = 0;
        let len = text.len();
        let text_bytes = text.as_bytes();
        self.wasm_functions.clear_shared_bytes(len).unwrap();
        while index < len {
            let write_count = std::cmp::min(len - index, self.buffer_size);
            self.write_bytes_to_memory_buffer(&text_bytes[index..(index + write_count)]);
            self.wasm_functions.add_to_shared_bytes_from_buffer(write_count).unwrap();
            index += write_count;
        }
    }

    fn write_bytes_to_memory_buffer(&self, bytes: &[u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr().unwrap();
        let memory_writer = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            memory_writer[i].set(bytes[i]);
        }
    }

    fn receive_string(&self, len: usize) -> Result<String, ErrBox> {
        let mut index = 0;
        let mut bytes: Vec<u8> = vec![0; len];
        while index < len {
            let read_count = std::cmp::min(len - index, self.buffer_size);
            self.wasm_functions.set_buffer_with_shared_bytes(index, read_count).unwrap();
            self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)]);
            index += read_count;
        }
        Ok(String::from_utf8(bytes)?)
    }

    fn read_bytes_from_memory_buffer(&self, bytes: &mut [u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr().unwrap();
        let memory_reader = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            bytes[i] = memory_reader[i].get();
        }
    }
}

impl InitializedPlugin for InitializedWasmPlugin {
    fn get_license_text(&self) -> Result<String, ErrBox> {
        let len = self.wasm_functions.get_license_text()?;
        self.receive_string(len)
    }

    fn get_resolved_config(&self) -> Result<String, ErrBox> {
        let len = self.wasm_functions.get_resolved_config()?;
        self.receive_string(len)
    }

    fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        let len = self.wasm_functions.get_config_diagnostics()?;
        let json_text = self.receive_string(len)?;
        Ok(serde_json::from_str(&json_text)?)
    }

    fn format_text(&self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> Result<String, ErrBox> {
        // send override config if necessary
        if !override_config.is_empty() {
            self.send_string(&serde_json::to_string(override_config)?);
            self.wasm_functions.set_override_config()?;
        }

        // send file path
        self.send_string(&file_path.to_string_lossy());
        self.wasm_functions.set_file_path()?;

        // send file text and format
        self.send_string(file_text);
        let response_code = self.wasm_functions.format()?;

        // handle the response
        match response_code {
            FormatResult::NoChange => Ok(String::from(file_text)),
            FormatResult::Change => {
                let len = self.wasm_functions.get_formatted_text()?;
                Ok(self.receive_string(len)?)
            }
            FormatResult::Error => {
                let len = self.wasm_functions.get_error_text()?;
                err!("{}", self.receive_string(len)?)
            }
        }
    }
}
