use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatError;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use dprint_core::plugins::PluginInfo;
use serde::Serialize;
use wasmtime::Caller;
use wasmtime::Engine;
use wasmtime::Memory;
use wasmtime::TypedFunc;
use wasmtime::WasmParams;
use wasmtime::WasmResults;

use crate::plugins::FormatConfig;
use crate::plugins::implementations::wasm::WasmHostFormatSender;
use crate::plugins::implementations::wasm::WasmInstance;

use super::InitializedWasmPluginInstance;
use super::Linker;
use super::Store;
use super::WasmHostState;

enum WasmFormatResult {
  NoChange,
  Change,
  Error,
}

#[derive(Clone, Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
struct SyncPluginInfo {
  #[serde(flatten)]
  pub info: PluginInfo,
  #[serde(flatten)]
  pub file_matching: FileMatchingInfo,
}

#[derive(Default)]
struct SharedBytes {
  data: Vec<u8>,
  index: usize,
}

impl SharedBytes {
  pub fn with_size(size: usize) -> Self {
    Self::from_bytes(vec![0; size])
  }

  pub fn from_bytes(data: Vec<u8>) -> Self {
    Self { data, index: 0 }
  }
}

/// The host state for a v3 plugin, stored in the wasmtime `Store` data.
pub struct ImportObjectEnvironmentV3 {
  pub memory: Option<Memory>,
  pub token: Arc<dyn CancellationToken>,
  override_config: Option<ConfigKeyMap>,
  file_path: Option<PathBuf>,
  formatted_text_store: Vec<u8>,
  shared_bytes: SharedBytes,
  error_text_store: String,
  host_format_sender: WasmHostFormatSender,
}

impl ImportObjectEnvironmentV3 {
  fn take_shared_bytes(&mut self) -> Vec<u8> {
    let data = std::mem::take(&mut self.shared_bytes.data);
    self.shared_bytes.index = 0;
    data
  }
}

pub fn add_identity_imports(linker: &mut Linker) -> Result<()> {
  linker.func_wrap("dprint", "host_clear_bytes", |_: u32| {})?;
  linker.func_wrap("dprint", "host_read_buffer", |_: u32, _: u32| {})?;
  linker.func_wrap("dprint", "host_write_buffer", |_: u32, _: u32, _: u32| {})?;
  linker.func_wrap("dprint", "host_take_override_config", || {})?;
  linker.func_wrap("dprint", "host_take_file_path", || {})?;
  linker.func_wrap("dprint", "host_format", || -> u32 { 0 })?; // no change
  linker.func_wrap("dprint", "host_get_formatted_text", || -> u32 { 0 })?; // zero length
  linker.func_wrap("dprint", "host_get_error_text", || -> u32 { 0 })?; // zero length
  Ok(())
}

pub fn create_pools_import_object(engine: &Engine, host_format_sender: WasmHostFormatSender) -> Result<(Linker, WasmHostState)> {
  let state = ImportObjectEnvironmentV3 {
    memory: None,
    token: Arc::new(NullCancellationToken),
    override_config: None,
    file_path: None,
    formatted_text_store: Default::default(),
    shared_bytes: SharedBytes::default(),
    error_text_store: Default::default(),
    host_format_sender,
  };
  let mut linker = Linker::new(engine);
  linker.func_wrap("dprint", "host_clear_bytes", host_clear_bytes)?;
  linker.func_wrap("dprint", "host_read_buffer", host_read_buffer)?;
  linker.func_wrap("dprint", "host_write_buffer", host_write_buffer)?;
  linker.func_wrap("dprint", "host_take_override_config", host_take_override_config)?;
  linker.func_wrap("dprint", "host_take_file_path", host_take_file_path)?;
  linker.func_wrap("dprint", "host_format", host_format)?;
  linker.func_wrap("dprint", "host_get_formatted_text", host_get_formatted_text)?;
  linker.func_wrap("dprint", "host_get_error_text", host_get_error_text)?;
  Ok((linker, WasmHostState::V3(state)))
}

fn env<'a>(caller: &'a Caller<'_, WasmHostState>) -> &'a ImportObjectEnvironmentV3 {
  match caller.data() {
    WasmHostState::V3(state) => state,
    _ => unreachable!("expected v3 host state"),
  }
}

fn env_mut<'a>(caller: &'a mut Caller<'_, WasmHostState>) -> &'a mut ImportObjectEnvironmentV3 {
  match caller.data_mut() {
    WasmHostState::V3(state) => state,
    _ => unreachable!("expected v3 host state"),
  }
}

fn host_clear_bytes(mut caller: Caller<'_, WasmHostState>, length: u32) {
  env_mut(&mut caller).shared_bytes = SharedBytes::with_size(length as usize);
}

fn host_read_buffer(mut caller: Caller<'_, WasmHostState>, buffer_pointer: u32, length: u32) {
  let memory = env(&caller).memory.unwrap();
  let length = length as usize;
  let mut tmp = vec![0u8; length];
  memory.read(&caller, buffer_pointer as usize, &mut tmp).unwrap();
  let env = env_mut(&mut caller);
  let index = env.shared_bytes.index;
  env.shared_bytes.data[index..index + length].copy_from_slice(&tmp);
  env.shared_bytes.index += length;
}

fn host_write_buffer(mut caller: Caller<'_, WasmHostState>, buffer_pointer: u32, offset: u32, length: u32) {
  let memory = env(&caller).memory.unwrap();
  let offset = offset as usize;
  let length = length as usize;
  let chunk = env(&caller).shared_bytes.data[offset..offset + length].to_vec();
  memory.write(&mut caller, buffer_pointer as usize, &chunk).unwrap();
}

fn host_take_override_config(mut caller: Caller<'_, WasmHostState>) {
  let env = env_mut(&mut caller);
  let bytes = env.take_shared_bytes();
  let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or_default();
  env.override_config.replace(config_key_map);
}

fn host_take_file_path(mut caller: Caller<'_, WasmHostState>) {
  let env = env_mut(&mut caller);
  let bytes = env.take_shared_bytes();
  let file_path_str = String::from_utf8(bytes).unwrap();
  env.file_path.replace(PathBuf::from(file_path_str));
}

fn host_format(mut caller: Caller<'_, WasmHostState>) -> u32 {
  let (override_config, file_path, file_bytes, token, host_format_sender) = {
    let env = env_mut(&mut caller);
    let override_config = env.override_config.take().unwrap_or_default();
    let file_path = env.file_path.take().expect("Expected to have file path.");
    let file_bytes = env.take_shared_bytes();
    (override_config, file_path, file_bytes, env.token.clone(), env.host_format_sender.clone())
  };
  let request = HostFormatRequest {
    file_path,
    file_bytes,
    range: None,
    override_config,
    token,
  };
  // todo: worth it to use a oneshot channel library here?
  let (tx, rx) = std::sync::mpsc::channel();
  let result = match host_format_sender.send((request, tx)) {
    Ok(()) => match rx.recv() {
      Ok(result) => result,
      Err(_) => Ok(None), // receive error
    },
    Err(_) => Ok(None), // send error
  };

  let env = env_mut(&mut caller);
  match result {
    Ok(Some(formatted_text)) => {
      env.formatted_text_store = formatted_text;
      1 // change
    }
    Ok(None) => {
      0 // no change
    }
    // ignore critical error as we can just continue formatting
    Err(err) => {
      env.error_text_store = err.to_string();
      2 // error
    }
  }
}

fn host_get_formatted_text(mut caller: Caller<'_, WasmHostState>) -> u32 {
  let env = env_mut(&mut caller);
  let formatted_bytes = std::mem::take(&mut env.formatted_text_store);
  let len = formatted_bytes.len();
  env.shared_bytes = SharedBytes::from_bytes(formatted_bytes);
  len as u32
}

fn host_get_error_text(mut caller: Caller<'_, WasmHostState>) -> u32 {
  let env = env_mut(&mut caller);
  let error_text = std::mem::take(&mut env.error_text_store);
  let len = error_text.len();
  env.shared_bytes = SharedBytes::from_bytes(error_text.into_bytes());
  len as u32
}

pub struct InitializedWasmPluginInstanceV3 {
  wasm_functions: WasmFunctions,
  buffer_size: usize,
  current_config_id: FormatConfigId,
}

impl InitializedWasmPluginInstanceV3 {
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
        Ok(Err(FormatError::new(text)))
      }
    }
  }

  fn ensure_config(&mut self, config: &FormatConfig) -> Result<()> {
    if self.current_config_id != config.id {
      // set this to uninitialized in case it errors below
      self.current_config_id = FormatConfigId::uninitialized();
      // update the plugin
      self.set_global_config(&config.global)?;
      self.set_plugin_config(&config.plugin)?;
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
    self.wasm_functions.write_memory(wasm_buffer_pointer as usize, bytes)?;
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
    self.wasm_functions.read_memory(wasm_buffer_pointer as usize, bytes)?;
    Ok(())
  }
}

impl InitializedWasmPluginInstance for InitializedWasmPluginInstanceV3 {
  fn plugin_info(&mut self) -> Result<PluginInfo> {
    self.sync_plugin_info().map(|i| i.info)
  }

  fn license_text(&mut self) -> Result<String> {
    let len = self.wasm_functions.get_license_text()?;
    self.receive_string(len)
  }

  fn check_config_updates(&mut self, _message: &CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    Ok(Vec::new())
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

  fn format_text(
    &mut self,
    file_path: &Path,
    file_bytes: &[u8],
    range: FormatRange,
    config: &FormatConfig,
    override_config: &ConfigKeyMap,
    token: Arc<dyn CancellationToken>,
  ) -> FormatResult {
    if range.is_some() && range != Some(0..file_bytes.len()) {
      return Ok(None); // not supported for v3
    }
    self.wasm_functions.instance.set_token(&mut self.wasm_functions.store, token);
    self.ensure_config(config).map_err(FormatError::new)?;
    match self.inner_format_text(file_path, file_bytes, override_config) {
      Ok(inner) => inner,
      Err(err) => Err(CriticalFormatError(FormatError::new(err)).into()),
    }
  }
}

struct WasmFunctions {
  store: Store,
  instance: WasmInstance,
  memory: Memory,
}

impl WasmFunctions {
  pub fn new(mut store: Store, instance: WasmInstance) -> Result<Self> {
    let memory = instance
      .get_memory(&mut store, "memory")
      .ok_or_else(|| anyhow!("Could not find memory export in plugin."))?;
    Ok(WasmFunctions { instance, memory, store })
  }

  #[inline]
  pub fn set_global_config(&mut self) -> Result<()> {
    let func = self.get_export::<(), ()>("set_global_config")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn set_plugin_config(&mut self) -> Result<()> {
    let func = self.get_export::<(), ()>("set_plugin_config")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn get_plugin_info(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_plugin_info")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_license_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_license_text")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_resolved_config(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_resolved_config")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_config_diagnostics(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_config_diagnostics")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn set_override_config(&mut self) -> Result<()> {
    let func = self.get_export::<(), ()>("set_override_config")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn set_file_path(&mut self) -> Result<()> {
    let func = self.get_export::<(), ()>("set_file_path")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn format(&mut self) -> Result<WasmFormatResult> {
    let func = self.get_export::<(), u32>("format")?;
    Ok(func.call(&mut self.store, ()).map(|value| u8_to_format_result(value as u8))?)
  }

  #[inline]
  pub fn get_formatted_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_formatted_text")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_error_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_error_text")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn clear_shared_bytes(&mut self, capacity: usize) -> Result<()> {
    let func = self.get_export::<u32, ()>("clear_shared_bytes")?;
    Ok(func.call(&mut self.store, capacity as u32)?)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_size(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_wasm_memory_buffer_size")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_wasm_memory_buffer_ptr(&mut self) -> Result<u32> {
    let func = self.get_export::<(), u32>("get_wasm_memory_buffer")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn set_buffer_with_shared_bytes(&mut self, offset: usize, length: usize) -> Result<()> {
    let func = self.get_export::<(u32, u32), ()>("set_buffer_with_shared_bytes")?;
    Ok(func.call(&mut self.store, (offset as u32, length as u32))?)
  }

  #[inline]
  pub fn add_to_shared_bytes_from_buffer(&mut self, length: usize) -> Result<()> {
    let func = self.get_export::<u32, ()>("add_to_shared_bytes_from_buffer")?;
    Ok(func.call(&mut self.store, length as u32)?)
  }

  #[inline]
  fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
    let memory = self.memory;
    memory.write(&mut self.store, offset, bytes)?;
    Ok(())
  }

  #[inline]
  fn read_memory(&mut self, offset: usize, bytes: &mut [u8]) -> Result<()> {
    let memory = self.memory;
    memory.read(&self.store, offset, bytes)?;
    Ok(())
  }

  fn get_export<P, R>(&mut self, name: &str) -> Result<TypedFunc<P, R>>
  where
    P: WasmParams,
    R: WasmResults,
  {
    match self.instance.get_function(&mut self.store, name) {
      Some(func) => match func.typed::<P, R>(&self.store) {
        Ok(typed_func) => Ok(typed_func),
        Err(err) => bail!("Error creating function '{}'. Message: {:#}", name, err),
      },
      None => bail!("Could not find export in plugin with name '{}'.", name),
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
