use std::path::Path;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::SyncPluginInfo;
use wasmer::Engine;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::MemoryView;
use wasmer::Store;
use wasmer::TypedFunction;
use wasmer::WasmPtr;
use wasmer::WasmTypeList;

use crate::plugins::implementations::wasm::WasmInstance;
use crate::plugins::FormatConfig;

use super::InitializedWasmPluginInstance;

enum WasmFormatResult {
  NoChange,
  Change,
  Error,
}

pub struct InitializedWasmPluginInstanceV1 {
  wasm_functions: WasmFunctions,
  buffer_size: usize,
  current_config_id: FormatConfigId,
}

impl InitializedWasmPluginInstanceV1 {
  pub fn new(store: Store, instance: WasmInstance) -> Result<Self> {
    let mut wasm_functions = WasmFunctions::new(store, instance)?;
    let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;
    Ok(Self {
      wasm_functions,
      buffer_size,
      current_config_id: FormatConfigId::uninitialized(),
    })
  }

  fn set_global_config(&mut self, global_config: &GlobalConfiguration) -> Result<()> {
    let json = serde_json::to_string(global_config)?;
    self.send_string(&json)?;
    self.wasm_functions.set_global_config()?;
    Ok(())
  }

  fn set_plugin_config(&mut self, plugin_config: &ConfigKeyMap) -> Result<()> {
    let json = serde_json::to_string(plugin_config)?;
    self.send_string(&json)?;
    self.wasm_functions.set_plugin_config()?;
    Ok(())
  }

  fn sync_plugin_info(&mut self) -> Result<SyncPluginInfo> {
    let len = self.wasm_functions.get_plugin_info()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn inner_format_text(&mut self, file_path: &Path, file_bytes: &[u8], override_config: &ConfigKeyMap) -> Result<FormatResult> {
    // send override config if necessary
    if !override_config.is_empty() {
      self.send_string(&match serde_json::to_string(override_config) {
        Ok(text) => text,
        Err(err) => return Ok(Err(err.into())),
      })?;
      self.wasm_functions.set_override_config()?;
    }

    // send file path
    self.send_string(&file_path.to_string_lossy())?;
    self.wasm_functions.set_file_path()?;

    // send file text and format
    self.send_bytes(file_bytes)?;
    let response_code = self.wasm_functions.format()?;

    // handle the response
    match response_code {
      WasmFormatResult::NoChange => Ok(Ok(None)),
      WasmFormatResult::Change => {
        let len = self.wasm_functions.get_formatted_text()?;
        let text_bytes = self.receive_bytes(len)?;
        Ok(Ok(Some(text_bytes)))
      }
      WasmFormatResult::Error => {
        let len = self.wasm_functions.get_error_text()?;
        let text = self.receive_string(len)?;
        Ok(Err(anyhow!("{}", text)))
      }
    }
  }

  fn ensure_config(&mut self, config: &FormatConfig) -> Result<()> {
    if self.current_config_id != config.id {
      // set this to uninitialized in case it errors below
      self.current_config_id = FormatConfigId::uninitialized();
      // update the plugin
      self.set_global_config(&config.global)?;
      self.set_plugin_config(&config.raw)?;
      // now mark this as successfully set
      self.current_config_id = config.id;
    }
    Ok(())
  }

  /* LOW LEVEL SENDING AND RECEIVING */

  // These methods should panic when failing because that may indicate
  // a major problem where the CLI is out of sync with the plugin.

  fn send_string(&mut self, text: &str) -> Result<()> {
    self.send_bytes(text.as_bytes())
  }

  fn send_bytes(&mut self, bytes: &[u8]) -> Result<()> {
    let mut index = 0;
    let len = bytes.len();
    self.wasm_functions.clear_shared_bytes(len)?;
    while index < len {
      let write_count = std::cmp::min(len - index, self.buffer_size);
      self.write_bytes_to_memory_buffer(&bytes[index..(index + write_count)])?;
      self.wasm_functions.add_to_shared_bytes_from_buffer(write_count)?;
      index += write_count;
    }
    Ok(())
  }

  fn write_bytes_to_memory_buffer(&mut self, bytes: &[u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.write(wasm_buffer_pointer.offset() as u64, bytes)?;
    Ok(())
  }

  fn receive_string(&mut self, len: usize) -> Result<String> {
    let bytes = self.receive_bytes(len)?;
    Ok(String::from_utf8(bytes)?)
  }

  fn receive_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
    let mut index = 0;
    let mut bytes: Vec<u8> = vec![0; len];
    while index < len {
      let read_count = std::cmp::min(len - index, self.buffer_size);
      self.wasm_functions.set_buffer_with_shared_bytes(index, read_count)?;
      self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)])?;
      index += read_count;
    }
    Ok(bytes)
  }

  fn read_bytes_from_memory_buffer(&mut self, bytes: &mut [u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.read(wasm_buffer_pointer.offset() as u64, bytes)?;
    Ok(())
  }
}

impl InitializedWasmPluginInstance for InitializedWasmPluginInstanceV1 {
  fn plugin_info(&mut self) -> Result<PluginInfo> {
    self.sync_plugin_info().map(|i| i.info)
  }

  fn license_text(&mut self) -> Result<String> {
    let len = self.wasm_functions.get_license_text()?;
    self.receive_string(len)
  }

  fn resolved_config(&mut self, config: &FormatConfig) -> Result<String> {
    self.ensure_config(config)?;
    let len = self.wasm_functions.get_resolved_config()?;
    self.receive_string(len)
  }

  fn config_diagnostics(&mut self, config: &FormatConfig) -> Result<Vec<ConfigurationDiagnostic>> {
    self.ensure_config(config)?;
    let len = self.wasm_functions.get_config_diagnostics()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn file_matching_info(&mut self, _config: &FormatConfig) -> Result<FileMatchingInfo> {
    self.sync_plugin_info().map(|i| i.file_matching)
  }

  fn format_text(&mut self, file_path: &Path, file_bytes: &[u8], config: &FormatConfig, override_config: &ConfigKeyMap) -> FormatResult {
    self.ensure_config(config)?;
    match self.inner_format_text(file_path, file_bytes, override_config) {
      Ok(inner) => inner,
      Err(err) => Err(CriticalFormatError(err).into()),
    }
  }
}

struct WasmFunctions {
  store: Store,
  instance: Instance,
  memory: Memory,
  // keep this alive for the duration of the engine otherwise it
  // could be cleaned up before the instance is dropped
  _engine: Engine,
}

impl WasmFunctions {
  pub fn new(store: Store, instance: WasmInstance) -> Result<Self> {
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
    Ok(set_global_config_func.call(&mut self.store)?)
  }

  #[inline]
  pub fn set_plugin_config(&mut self) -> Result<()> {
    let set_plugin_config_func = self.get_export::<(), ()>("set_plugin_config")?;
    Ok(set_plugin_config_func.call(&mut self.store)?)
  }

  #[inline]
  pub fn get_plugin_info(&mut self) -> Result<usize> {
    let get_plugin_info_func = self.get_export::<(), u32>("get_plugin_info")?;
    Ok(get_plugin_info_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_license_text(&mut self) -> Result<usize> {
    let get_license_text_func = self.get_export::<(), u32>("get_license_text")?;
    Ok(get_license_text_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_resolved_config(&mut self) -> Result<usize> {
    let get_resolved_config_func = self.get_export::<(), u32>("get_resolved_config")?;
    Ok(get_resolved_config_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_config_diagnostics(&mut self) -> Result<usize> {
    let get_config_diagnostics_func = self.get_export::<(), u32>("get_config_diagnostics")?;
    Ok(get_config_diagnostics_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn set_override_config(&mut self) -> Result<()> {
    let set_override_config_func = self.get_export::<(), ()>("set_override_config")?;
    Ok(set_override_config_func.call(&mut self.store)?)
  }

  #[inline]
  pub fn set_file_path(&mut self) -> Result<()> {
    let set_file_path_func = self.get_export::<(), ()>("set_file_path")?;
    Ok(set_file_path_func.call(&mut self.store)?)
  }

  #[inline]
  pub fn format(&mut self) -> Result<WasmFormatResult> {
    let format_func = self.get_export::<(), u8>("format")?;
    Ok(format_func.call(&mut self.store).map(u8_to_format_result)?)
  }

  #[inline]
  pub fn get_formatted_text(&mut self) -> Result<usize> {
    let get_formatted_text_func = self.get_export::<(), u32>("get_formatted_text")?;
    Ok(get_formatted_text_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_error_text(&mut self) -> Result<usize> {
    let get_error_text_func = self.get_export::<(), u32>("get_error_text")?;
    Ok(get_error_text_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_memory_view(&self) -> MemoryView {
    self.memory.view(&self.store)
  }

  #[inline]
  pub fn clear_shared_bytes(&mut self, capacity: usize) -> Result<()> {
    let clear_shared_bytes_func = self.get_export::<u32, ()>("clear_shared_bytes")?;
    Ok(clear_shared_bytes_func.call(&mut self.store, capacity as u32)?)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_size(&mut self) -> Result<usize> {
    let get_wasm_memory_buffer_size_func = self.get_export::<(), u32>("get_wasm_memory_buffer_size")?;
    Ok(get_wasm_memory_buffer_size_func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_ptr(&mut self) -> Result<WasmPtr<u32>> {
    let get_wasm_memory_buffer_func = self.get_export::<(), WasmPtr<u32>>("get_wasm_memory_buffer")?;
    Ok(get_wasm_memory_buffer_func.call(&mut self.store)?)
  }

  #[inline]
  pub fn set_buffer_with_shared_bytes(&mut self, offset: usize, length: usize) -> Result<()> {
    let set_buffer_with_shared_bytes_func = self.get_export::<(u32, u32), ()>("set_buffer_with_shared_bytes")?;
    Ok(set_buffer_with_shared_bytes_func.call(&mut self.store, offset as u32, length as u32)?)
  }

  #[inline]
  pub fn add_to_shared_bytes_from_buffer(&mut self, length: usize) -> Result<()> {
    let add_to_shared_bytes_from_buffer_func = self.get_export::<u32, ()>("add_to_shared_bytes_from_buffer")?;

    Ok(add_to_shared_bytes_from_buffer_func.call(&mut self.store, length as u32)?)
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

fn u8_to_format_result(orig: u8) -> WasmFormatResult {
  match orig {
    0 => WasmFormatResult::NoChange,
    1 => WasmFormatResult::Change,
    2 => WasmFormatResult::Error,
    _ => unreachable!(),
  }
}
