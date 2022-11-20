use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use parking_lot::Mutex;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::MemoryView;
use wasmer::RuntimeError;
use wasmer::Store;
use wasmer::TypedFunction;
use wasmer::WasmPtr;
use wasmer::WasmTypeList;

use dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION;

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
  store: Arc<Mutex<Store>>,
  instance: Instance,
  memory: Memory,
}

impl WasmFunctions {
  pub fn new(store: Arc<Mutex<Store>>, instance: Instance) -> Result<Self> {
    match get_plugin_schema_version(&mut *store.lock(), &instance) {
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
    let memory = instance.exports.get_memory("memory")?.clone();

    Ok(WasmFunctions { instance, memory, store })
  }

  #[inline]
  pub fn set_global_config(&self) -> Result<()> {
    let set_global_config_func = self.get_export::<(), ()>("set_global_config")?;
    wasm_runtime_error_to_err_box(set_global_config_func.call(&mut *self.store.lock()))
  }

  #[inline]
  pub fn set_plugin_config(&self) -> Result<()> {
    let set_plugin_config_func = self.get_export::<(), ()>("set_plugin_config")?;
    wasm_runtime_error_to_err_box(set_plugin_config_func.call(&mut *self.store.lock()))
  }

  #[inline]
  pub fn get_plugin_info(&self) -> Result<usize> {
    let get_plugin_info_func = self.get_export::<(), u32>("get_plugin_info")?;
    wasm_runtime_error_to_err_box(get_plugin_info_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_license_text(&self) -> Result<usize> {
    let get_license_text_func = self.get_export::<(), u32>("get_license_text")?;
    wasm_runtime_error_to_err_box(get_license_text_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_resolved_config(&self) -> Result<usize> {
    let get_resolved_config_func = self.get_export::<(), u32>("get_resolved_config")?;
    wasm_runtime_error_to_err_box(get_resolved_config_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_config_diagnostics(&self) -> Result<usize> {
    let get_config_diagnostics_func = self.get_export::<(), u32>("get_config_diagnostics")?;
    wasm_runtime_error_to_err_box(get_config_diagnostics_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn set_override_config(&self) -> Result<()> {
    let set_override_config_func = self.get_export::<(), ()>("set_override_config")?;
    wasm_runtime_error_to_err_box(set_override_config_func.call(&mut *self.store.lock()))
  }

  #[inline]
  pub fn set_file_path(&self) -> Result<()> {
    let set_file_path_func = self.get_export::<(), ()>("set_file_path")?;
    wasm_runtime_error_to_err_box(set_file_path_func.call(&mut *self.store.lock()))
  }

  #[inline]
  pub fn format(&self) -> Result<WasmFormatResult> {
    let format_func = self.get_export::<(), u8>("format")?;
    wasm_runtime_error_to_err_box(format_func.call(&mut *self.store.lock())).map(|value| value.into())
  }

  #[inline]
  pub fn get_formatted_text(&self) -> Result<usize> {
    let get_formatted_text_func = self.get_export::<(), u32>("get_formatted_text")?;
    wasm_runtime_error_to_err_box(get_formatted_text_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_error_text(&self) -> Result<usize> {
    let get_error_text_func = self.get_export::<(), u32>("get_error_text")?;
    wasm_runtime_error_to_err_box(get_error_text_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_memory_view(&self) -> MemoryView {
    self.memory.view(&*self.store.lock())
  }

  #[inline]
  pub fn clear_shared_bytes(&self, capacity: usize) -> Result<()> {
    let clear_shared_bytes_func = self.get_export::<u32, ()>("clear_shared_bytes")?;
    wasm_runtime_error_to_err_box(clear_shared_bytes_func.call(&mut *self.store.lock(), capacity as u32))
  }

  #[inline]
  pub fn get_wasm_memory_buffer_size(&self) -> Result<usize> {
    let get_wasm_memory_buffer_size_func = self.get_export::<(), u32>("get_wasm_memory_buffer_size")?;
    wasm_runtime_error_to_err_box(get_wasm_memory_buffer_size_func.call(&mut *self.store.lock())).map(|value| value as usize)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_ptr(&self) -> Result<WasmPtr<u32>> {
    let get_wasm_memory_buffer_func = self.get_export::<(), WasmPtr<u32>>("get_wasm_memory_buffer")?;
    wasm_runtime_error_to_err_box(get_wasm_memory_buffer_func.call(&mut *self.store.lock()))
  }

  #[inline]
  pub fn set_buffer_with_shared_bytes(&self, offset: usize, length: usize) -> Result<()> {
    let set_buffer_with_shared_bytes_func = self.get_export::<(u32, u32), ()>("set_buffer_with_shared_bytes")?;
    wasm_runtime_error_to_err_box(set_buffer_with_shared_bytes_func.call(&mut *self.store.lock(), offset as u32, length as u32))
  }

  #[inline]
  pub fn add_to_shared_bytes_from_buffer(&self, length: usize) -> Result<()> {
    let add_to_shared_bytes_from_buffer_func = self.get_export::<u32, ()>("add_to_shared_bytes_from_buffer")?;

    wasm_runtime_error_to_err_box(add_to_shared_bytes_from_buffer_func.call(&mut *self.store.lock(), length as u32))
  }

  fn get_export<Args, Rets>(&self, name: &str) -> Result<TypedFunction<Args, Rets>>
  where
    Args: WasmTypeList,
    Rets: WasmTypeList,
  {
    match self.instance.exports.get_function(name) {
      Ok(func) => match func.typed::<Args, Rets>(&mut *self.store.lock()) {
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
