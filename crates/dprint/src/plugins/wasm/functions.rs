use wasmer_runtime_core::export::Exportable;
use wasmer_runtime::{Instance, Func, WasmPtr, Array, Memory};

use super::super::super::types::ErrBox;

const CURRENT_SCHEMA_VERSION: u32 = 1;

pub enum FormatResult {
    NoChange = 0,
    Change = 1,
    Error = 2,
}

impl From<u8> for FormatResult {
    fn from(orig: u8) -> Self {
        match orig {
            0 => FormatResult::NoChange,
            1 => FormatResult::Change,
            2 => FormatResult::Error,
            _ => unreachable!(),
        }
    }
}

pub struct WasmFunctions {
    instance: Instance,
}

impl WasmFunctions {
    pub fn new(instance: Instance) -> Result<Self, ErrBox> {
        let plugin_schema_version_func: Func<(), u32> = instance.exports.get("get_plugin_schema_version")?;
        let plugin_schema_version = plugin_schema_version_func.call().unwrap();

        if plugin_schema_version != CURRENT_SCHEMA_VERSION {
            return err!(
                "Invalid schema version: {} -- Expected: {}. This may indicate you should upgrade your Dprint cli",
                plugin_schema_version,
                CURRENT_SCHEMA_VERSION
            );
        }

        Ok(WasmFunctions { instance })
    }

    #[inline]
    pub fn set_global_config(&self) {
        let set_global_config_func: Func = self.get_export("set_global_config");
        set_global_config_func.call().unwrap()
    }

    #[inline]
    pub fn set_plugin_config(&self) {
        let set_plugin_config_func: Func = self.get_export("set_plugin_config");
        set_plugin_config_func.call().unwrap()
    }

    #[inline]
    pub fn get_plugin_info(&self) -> usize {
        let get_plugin_info_func: Func<(), u32> = self.get_export("get_plugin_info");
        get_plugin_info_func.call().unwrap() as usize
    }

    #[inline]
    pub fn get_resolved_config(&self) -> usize {
        let get_resolved_config_func: Func<(), u32> = self.get_export("get_resolved_config");
        get_resolved_config_func.call().unwrap() as usize
    }

    #[inline]
    pub fn get_config_diagnostics(&self) -> usize {
        let get_config_diagnostics_func: Func<(), u32> = self.get_export("get_config_diagnostics");
        get_config_diagnostics_func.call().unwrap() as usize
    }

    #[inline]
    pub fn set_file_path(&self) {
        let set_file_path_func: Func = self.get_export("set_file_path");
        set_file_path_func.call().unwrap()
    }

    #[inline]
    pub fn format(&self) -> FormatResult {
        let format_func: Func<(), u8> = self.get_export("format");
        format_func.call().unwrap().into()
    }

    #[inline]
    pub fn get_formatted_text(&self) -> usize {
        let get_formatted_text_func: Func<(), u32> = self.get_export("get_formatted_text");
        get_formatted_text_func.call().unwrap() as usize
    }

    #[inline]
    pub fn get_error_text(&self) -> usize {
        let get_error_text_func: Func<(), u32> = self.get_export("get_error_text");
        get_error_text_func.call().unwrap() as usize
    }

    #[inline]
    pub fn get_memory(&self) -> &Memory {
        self.instance.context().memory(0)
    }

    #[inline]
    pub fn clear_shared_bytes(&self, capacity: usize) {
        let clear_shared_bytes_func: Func<u32> = self.get_export("clear_shared_bytes");
        clear_shared_bytes_func.call(capacity as u32).unwrap();
    }

    #[inline]
    pub fn get_wasm_memory_buffer_size(&self) -> usize {
        let get_wasm_memory_buffer_size_func: Func<(), u32> = self.get_export("get_wasm_memory_buffer_size");
        get_wasm_memory_buffer_size_func.call().unwrap() as usize
    }

    #[inline]
    pub fn get_wasm_memory_buffer_ptr(&self) -> WasmPtr<u8, Array> {
        let get_wasm_memory_buffer_func: Func<(), WasmPtr<u8, Array>> = self.get_export("get_wasm_memory_buffer");
        get_wasm_memory_buffer_func.call().unwrap()
    }

    #[inline]
    pub fn set_buffer_with_shared_bytes(&self, offset: usize, length: usize) {
        let set_buffer_with_shared_bytes_func: Func<(u32, u32)> = self.get_export("set_buffer_with_shared_bytes");
        set_buffer_with_shared_bytes_func.call(offset as u32, length as u32).unwrap();
    }

    #[inline]
    pub fn add_to_shared_bytes_from_buffer(&self, length: usize) {
        let add_to_shared_bytes_from_buffer_func: Func<u32> = self.get_export("add_to_shared_bytes_from_buffer");
        add_to_shared_bytes_from_buffer_func.call(length as u32).unwrap();
    }

    fn get_export<'a, T: Exportable<'a>>(&'a self, name: &str) -> T {
        self.instance.exports.get(name).expect("Expected to find plugin method.")
    }
}
