use anyhow::bail;
use anyhow::Result;
use wasmer::Engine;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::MemoryView;
use wasmer::RuntimeError;
use wasmer::Store;
use wasmer::TypedFunction;
use wasmer::WasmPtr;
use wasmer::WasmTypeList;

use dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION;

use super::load_instance::WasmInstance;

pub enum WasmFormatResult {
  NoChange = 0,
  Change = 1,
  Error = 2,
}

impl From<u8> for WasmFormatResult {
  fn from(orig: u8) -> Self {
    match orig {
      0 => WasmFormatResult::NoChange,
      1 => WasmFormatResult::Change,
      2 => WasmFormatResult::Error,
      _ => unreachable!(),
    }
  }
}

pub struct WasmFunctions {
  store: Store,
  instance: Instance,
  memory: Memory,
  // keep this alive for the duration of the engine otherwise it
  // could be cleaned up before the instance is dropped
  _engine: Engine,
}

impl WasmFunctions {
  pub fn new(mut store: Store, instance: WasmInstance) -> Result<Self> {
    match get_plugin_schema_version(&mut store, &instance.inner) {
      Ok(plugin_schema_version) => {
        if plugin_schema_version != PLUGIN_SYSTEM_SCHEMA_VERSION {
          bail!(
            "Invalid schema version: {} -- Expected: {}. This may indicate you should upgrade your dprint CLI or plugin.",
            plugin_schema_version,
            PLUGIN_SYSTEM_SCHEMA_VERSION
          );
        }
      }
      Err(err) => {
        bail!(
          "Error determining plugin schema version. Are you sure this is a dprint plugin? {}",
          err.to_string()
        );
      }
    }
    let memory = instance.inner.exports.get_memory("memory")?.clone();

    Ok(WasmFunctions {
      instance: instance.inner,
      memory,
      store,
      _engine: instance.engine,
    })
  }

  #[inline]
  pub fn set_global_config(&mut self) -> Result<()> {
    let set_global_config_func = self.get_export::<(), ()>("set_global_config")?;
    wasm_runtime_error_to_err_box(set_global_config_func.call(&mut self.store))
  }

  #[inline]
  pub fn set_plugin_config(&mut self) -> Result<()> {
    let set_plugin_config_func = self.get_export::<(), ()>("set_plugin_config")?;
    wasm_runtime_error_to_err_box(set_plugin_config_func.call(&mut self.store))
  }

  #[inline]
  pub fn get_plugin_info(&mut self) -> Result<usize> {
    let get_plugin_info_func = self.get_export::<(), u32>("get_plugin_info")?;
    wasm_runtime_error_to_err_box(get_plugin_info_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_license_text(&mut self) -> Result<usize> {
    let get_license_text_func = self.get_export::<(), u32>("get_license_text")?;
    wasm_runtime_error_to_err_box(get_license_text_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_resolved_config(&mut self) -> Result<usize> {
    let get_resolved_config_func = self.get_export::<(), u32>("get_resolved_config")?;
    wasm_runtime_error_to_err_box(get_resolved_config_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_config_diagnostics(&mut self) -> Result<usize> {
    let get_config_diagnostics_func = self.get_export::<(), u32>("get_config_diagnostics")?;
    wasm_runtime_error_to_err_box(get_config_diagnostics_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn set_override_config(&mut self) -> Result<()> {
    let set_override_config_func = self.get_export::<(), ()>("set_override_config")?;
    wasm_runtime_error_to_err_box(set_override_config_func.call(&mut self.store))
  }

  #[inline]
  pub fn set_file_path(&mut self) -> Result<()> {
    let set_file_path_func = self.get_export::<(), ()>("set_file_path")?;
    wasm_runtime_error_to_err_box(set_file_path_func.call(&mut self.store))
  }

  #[inline]
  pub fn format(&mut self) -> Result<WasmFormatResult> {
    let format_func = self.get_export::<(), u8>("format")?;
    wasm_runtime_error_to_err_box(format_func.call(&mut self.store)).map(|value| value.into())
  }

  #[inline]
  pub fn get_formatted_text(&mut self) -> Result<usize> {
    let get_formatted_text_func = self.get_export::<(), u32>("get_formatted_text")?;
    wasm_runtime_error_to_err_box(get_formatted_text_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_error_text(&mut self) -> Result<usize> {
    let get_error_text_func = self.get_export::<(), u32>("get_error_text")?;
    wasm_runtime_error_to_err_box(get_error_text_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_memory_view(&self) -> MemoryView {
    self.memory.view(&self.store)
  }

  #[inline]
  pub fn clear_shared_bytes(&mut self, capacity: usize) -> Result<()> {
    let clear_shared_bytes_func = self.get_export::<u32, ()>("clear_shared_bytes")?;
    wasm_runtime_error_to_err_box(clear_shared_bytes_func.call(&mut self.store, capacity as u32))
  }

  #[inline]
  pub fn get_wasm_memory_buffer_size(&mut self) -> Result<usize> {
    let get_wasm_memory_buffer_size_func = self.get_export::<(), u32>("get_wasm_memory_buffer_size")?;
    wasm_runtime_error_to_err_box(get_wasm_memory_buffer_size_func.call(&mut self.store)).map(|value| value as usize)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_ptr(&mut self) -> Result<WasmPtr<u32>> {
    let get_wasm_memory_buffer_func = self.get_export::<(), WasmPtr<u32>>("get_wasm_memory_buffer")?;
    wasm_runtime_error_to_err_box(get_wasm_memory_buffer_func.call(&mut self.store))
  }

  #[inline]
  pub fn set_buffer_with_shared_bytes(&mut self, offset: usize, length: usize) -> Result<()> {
    let set_buffer_with_shared_bytes_func = self.get_export::<(u32, u32), ()>("set_buffer_with_shared_bytes")?;
    wasm_runtime_error_to_err_box(set_buffer_with_shared_bytes_func.call(&mut self.store, offset as u32, length as u32))
  }

  #[inline]
  pub fn add_to_shared_bytes_from_buffer(&mut self, length: usize) -> Result<()> {
    let add_to_shared_bytes_from_buffer_func = self.get_export::<u32, ()>("add_to_shared_bytes_from_buffer")?;

    wasm_runtime_error_to_err_box(add_to_shared_bytes_from_buffer_func.call(&mut self.store, length as u32))
  }

  fn get_export<Args, Rets>(&mut self, name: &str) -> Result<TypedFunction<Args, Rets>>
  where
    Args: WasmTypeList,
    Rets: WasmTypeList,
  {
    match self.instance.exports.get_function(name) {
      Ok(func) => match func.typed::<Args, Rets>(&self.store) {
        Ok(native_func) => Ok(native_func),
        Err(err) => bail!("Error creating function '{}'. Message: {:#}", name, err),
      },
      Err(err) => bail!("Could not find export in plugin with name '{}'. Message: {:#}", name, err),
    }
  }
}

fn get_plugin_schema_version(store: &mut Store, instance: &Instance) -> Result<u32> {
  let plugin_schema_version_func = instance.exports.get_function("get_plugin_schema_version")?;
  let plugin_schema_version_func = plugin_schema_version_func.typed::<(), u32>(store)?;
  Ok(plugin_schema_version_func.call(store)?)
}

#[inline]
fn wasm_runtime_error_to_err_box<T>(result: Result<T, RuntimeError>) -> Result<T> {
  // need to do this because RuntimeError can't be sent between threads safely
  match result {
    Ok(value) => Ok(value),
    Err(err) => Err(err.into()),
  }
}
