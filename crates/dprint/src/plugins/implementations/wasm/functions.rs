use wasmer_runtime_core::export::Exportable;
use wasmer_runtime::{Instance, Func, WasmPtr, Array, Memory};

use dprint_core::types::{ErrBox, Error};
use dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION;

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

        if plugin_schema_version != PLUGIN_SYSTEM_SCHEMA_VERSION {
            return err!(
                "Invalid schema version: {} -- Expected: {}. This may indicate you should upgrade your dprint CLI or plugin.",
                plugin_schema_version,
                PLUGIN_SYSTEM_SCHEMA_VERSION
            );
        }

        Ok(WasmFunctions { instance })
    }

    #[inline]
    pub fn set_global_config(&self) -> Result<(), ErrBox> {
        let set_global_config_func: Func = self.get_export("set_global_config")?;
        wasm_runtime_error_to_err_box(set_global_config_func.call())
    }

    #[inline]
    pub fn set_plugin_config(&self) -> Result<(), ErrBox> {
        let set_plugin_config_func: Func = self.get_export("set_plugin_config")?;
        wasm_runtime_error_to_err_box(set_plugin_config_func.call())
    }

    #[inline]
    pub fn get_plugin_info(&self) -> Result<usize, ErrBox> {
        let get_plugin_info_func: Func<(), u32> = self.get_export("get_plugin_info")?;
        wasm_runtime_error_to_err_box(get_plugin_info_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_license_text(&self) -> Result<usize, ErrBox> {
        let get_license_text_func: Func<(), u32> = self.get_export("get_license_text")?;
        wasm_runtime_error_to_err_box(get_license_text_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_resolved_config(&self) -> Result<usize, ErrBox> {
        let get_resolved_config_func: Func<(), u32> = self.get_export("get_resolved_config")?;
        wasm_runtime_error_to_err_box(get_resolved_config_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_config_diagnostics(&self) -> Result<usize, ErrBox> {
        let get_config_diagnostics_func: Func<(), u32> = self.get_export("get_config_diagnostics")?;
        wasm_runtime_error_to_err_box(get_config_diagnostics_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn set_override_config(&self) -> Result<(), ErrBox> {
        let set_override_config_func: Func = self.get_export("set_override_config")?;
        wasm_runtime_error_to_err_box(set_override_config_func.call())
    }

    #[inline]
    pub fn set_file_path(&self) -> Result<(), ErrBox> {
        let set_file_path_func: Func = self.get_export("set_file_path")?;
        wasm_runtime_error_to_err_box(set_file_path_func.call())
    }

    #[inline]
    pub fn format(&self) -> Result<FormatResult, ErrBox> {
        let format_func: Func<(), u8> = self.get_export("format")?;
        wasm_runtime_error_to_err_box(format_func.call()).map(|value| value.into())
    }

    #[inline]
    pub fn get_formatted_text(&self) -> Result<usize, ErrBox> {
        let get_formatted_text_func: Func<(), u32> = self.get_export("get_formatted_text")?;
        wasm_runtime_error_to_err_box(get_formatted_text_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_error_text(&self) -> Result<usize, ErrBox> {
        let get_error_text_func: Func<(), u32> = self.get_export("get_error_text")?;
        wasm_runtime_error_to_err_box(get_error_text_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_memory(&self) -> &Memory {
        self.instance.context().memory(0)
    }

    #[inline]
    pub fn clear_shared_bytes(&self, capacity: usize) -> Result<(), ErrBox> {
        let clear_shared_bytes_func: Func<u32> = self.get_export("clear_shared_bytes")?;
        wasm_runtime_error_to_err_box(clear_shared_bytes_func.call(capacity as u32))
    }

    #[inline]
    pub fn get_wasm_memory_buffer_size(&self) -> Result<usize, ErrBox> {
        let get_wasm_memory_buffer_size_func: Func<(), u32> = self.get_export("get_wasm_memory_buffer_size")?;
        wasm_runtime_error_to_err_box(get_wasm_memory_buffer_size_func.call()).map(|value| value as usize)
    }

    #[inline]
    pub fn get_wasm_memory_buffer_ptr(&self) -> Result<WasmPtr<u8, Array>, ErrBox> {
        let get_wasm_memory_buffer_func: Func<(), WasmPtr<u8, Array>> = self.get_export("get_wasm_memory_buffer")?;
        wasm_runtime_error_to_err_box(get_wasm_memory_buffer_func.call())
    }

    #[inline]
    pub fn set_buffer_with_shared_bytes(&self, offset: usize, length: usize) -> Result<(), ErrBox> {
        let set_buffer_with_shared_bytes_func: Func<(u32, u32)> = self.get_export("set_buffer_with_shared_bytes")?;
        wasm_runtime_error_to_err_box(set_buffer_with_shared_bytes_func.call(offset as u32, length as u32))
    }

    #[inline]
    pub fn add_to_shared_bytes_from_buffer(&self, length: usize) -> Result<(), ErrBox> {
        let add_to_shared_bytes_from_buffer_func: Func<u32> = self.get_export("add_to_shared_bytes_from_buffer")?;
        wasm_runtime_error_to_err_box(add_to_shared_bytes_from_buffer_func.call(length as u32))
    }

    fn get_export<'a, T: Exportable<'a>>(&'a self, name: &str) -> Result<T, ErrBox> {
        match self.instance.exports.get(name) {
            Ok(export) => Ok(export),
            Err(err) => err!("Could not find export in plugin with name '{}'. Message: {}", name, err.to_string()),
        }
    }
}

#[inline]
fn wasm_runtime_error_to_err_box<T>(result: Result<T, wasmer_runtime::error::RuntimeError>) -> Result<T, ErrBox> {
    // need to do this because RuntimeError can't be sent between threads safely
    match result {
        Ok(value) => Ok(value),
        Err(err) => Err(Error::new(err.to_string())),
    }
}