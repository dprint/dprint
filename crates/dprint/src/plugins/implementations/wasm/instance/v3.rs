use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use dprint_core::plugins::PluginInfo;
use parking_lot::Mutex;
use serde::Serialize;
use wasmer::AsStoreRef;
use wasmer::ExportError;
use wasmer::Function;
use wasmer::FunctionEnv;
use wasmer::FunctionEnvMut;
use wasmer::Instance;
use wasmer::Memory;
use wasmer::MemoryView;
use wasmer::Store;
use wasmer::TypedFunction;
use wasmer::WasmPtr;
use wasmer::WasmTypeList;

use crate::plugins::implementations::wasm::WasmHostFormatSender;
use crate::plugins::implementations::wasm::WasmInstance;
use crate::plugins::FormatConfig;

use super::ImportObjectEnvironment;
use super::InitializedWasmPluginInstance;

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

pub fn create_identity_import_object(store: &mut Store) -> wasmer::Imports {
  let host_clear_bytes = |_: u32| {};
  let host_read_buffer = |_: u32, _: u32| {};
  let host_write_buffer = |_: u32, _: u32, _: u32| {};
  let host_take_override_config = || {};
  let host_take_file_path = || {};
  let host_format = || -> u32 { 0 }; // no change
  let host_get_formatted_text = || -> u32 { 0 }; // zero length
  let host_get_error_text = || -> u32 { 0 }; // zero length

  wasmer::imports! {
    "dprint" => {
      "host_clear_bytes" => Function::new_typed(store, host_clear_bytes),
      "host_read_buffer" => Function::new_typed(store, host_read_buffer),
      "host_write_buffer" => Function::new_typed(store, host_write_buffer),
      "host_take_override_config" => Function::new_typed(store, host_take_override_config),
      "host_take_file_path" => Function::new_typed(store, host_take_file_path),
      "host_format" => Function::new_typed(store, host_format),
      "host_get_formatted_text" => Function::new_typed(store, host_get_formatted_text),
      "host_get_error_text" => Function::new_typed(store, host_get_error_text),
    }
  }
}

pub fn create_pools_import_object(store: &mut Store, host_format_sender: WasmHostFormatSender) -> (wasmer::Imports, Box<dyn ImportObjectEnvironment>) {
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

  struct ImportObjectEnvironmentV3 {
    memory: Option<Memory>,
    override_config: Option<ConfigKeyMap>,
    file_path: Option<PathBuf>,
    formatted_text_store: Vec<u8>,
    shared_bytes: Mutex<SharedBytes>,
    error_text_store: String,
    token: Arc<dyn CancellationToken>,
    host_format_sender: WasmHostFormatSender,
  }

  impl ImportObjectEnvironmentV3 {
    pub fn new(host_format_sender: WasmHostFormatSender) -> Self {
      ImportObjectEnvironmentV3 {
        memory: None,
        override_config: None,
        file_path: None,
        shared_bytes: Mutex::new(SharedBytes::default()),
        formatted_text_store: Default::default(),
        error_text_store: Default::default(),
        token: Arc::new(NullCancellationToken),
        host_format_sender,
      }
    }

    fn take_shared_bytes(&self) -> Vec<u8> {
      let mut shared_bytes = self.shared_bytes.lock();
      let data = std::mem::take(&mut shared_bytes.data);
      shared_bytes.index = 0;
      data
    }
  }

  impl ImportObjectEnvironment for FunctionEnv<ImportObjectEnvironmentV3> {
    fn initialize(&self, store: &mut Store, instance: &Instance) -> Result<(), ExportError> {
      self.as_mut(store).memory = Some(instance.exports.get_memory("memory")?.clone());
      Ok(())
    }

    fn set_token(&self, store: &mut Store, token: Arc<dyn CancellationToken>) {
      // only used for host formatting in v3
      self.as_mut(store).token = token;
    }
  }

  fn host_clear_bytes(env: FunctionEnvMut<ImportObjectEnvironmentV3>, length: u32) {
    let env = env.data();
    *env.shared_bytes.lock() = SharedBytes::with_size(length as usize);
  }

  fn host_read_buffer(env: FunctionEnvMut<ImportObjectEnvironmentV3>, buffer_pointer: u32, length: u32) {
    let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);

    let length = length as usize;
    let mut shared_bytes = env_data.shared_bytes.lock();
    let shared_bytes_index = shared_bytes.index;
    memory_view
      .read(
        buffer_pointer.offset() as u64,
        &mut shared_bytes.data[shared_bytes_index..shared_bytes_index + length],
      )
      .unwrap();
    shared_bytes.index += length;
  }

  fn host_write_buffer(env: FunctionEnvMut<ImportObjectEnvironmentV3>, buffer_pointer: u32, offset: u32, length: u32) {
    let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);
    let offset = offset as usize;
    let length = length as usize;
    let shared_bytes = env_data.shared_bytes.lock();
    memory_view
      .write(buffer_pointer.offset() as u64, &shared_bytes.data[offset..offset + length])
      .unwrap();
  }

  fn host_take_override_config(mut env: FunctionEnvMut<ImportObjectEnvironmentV3>) {
    let env = env.data_mut();
    let bytes = env.take_shared_bytes();
    let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or_default();
    env.override_config.replace(config_key_map);
  }

  fn host_take_file_path(mut env: FunctionEnvMut<ImportObjectEnvironmentV3>) {
    let env = env.data_mut();
    let bytes = env.take_shared_bytes();
    let file_path_str = String::from_utf8(bytes).unwrap();
    env.file_path.replace(PathBuf::from(file_path_str));
  }

  fn host_format(mut env: FunctionEnvMut<ImportObjectEnvironmentV3>) -> u32 {
    let env = env.data_mut();
    let override_config = env.override_config.take().unwrap_or_default();
    let file_path = env.file_path.take().expect("Expected to have file path.");
    let file_bytes = env.take_shared_bytes();
    let request = HostFormatRequest {
      file_path,
      file_bytes,
      range: None,
      override_config,
      token: env.token.clone(),
    };
    // todo: worth it to use a oneshot channel library here?
    let (tx, rx) = std::sync::mpsc::channel();
    let send_result = env.host_format_sender.send((request, tx));
    let result = match send_result {
      Ok(()) => match rx.recv() {
        Ok(result) => result,
        Err(_) => {
          Ok(None) //receive error
        }
      },
      Err(_) => Ok(None), // send error
    };

    match result {
      Ok(Some(formatted_text)) => {
        //let mut env = env.data_mut();
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

  fn host_get_formatted_text(mut env: FunctionEnvMut<ImportObjectEnvironmentV3>) -> u32 {
    let env = env.data_mut();
    let formatted_bytes = std::mem::take(&mut env.formatted_text_store);
    let len = formatted_bytes.len();
    *env.shared_bytes.lock() = SharedBytes::from_bytes(formatted_bytes);
    len as u32
  }

  fn host_get_error_text(mut env: FunctionEnvMut<ImportObjectEnvironmentV3>) -> u32 {
    let env = env.data_mut();
    let error_text = std::mem::take(&mut env.error_text_store);
    let len = error_text.len();
    *env.shared_bytes.lock() = SharedBytes::from_bytes(error_text.into_bytes());
    len as u32
  }

  let env = ImportObjectEnvironmentV3::new(host_format_sender);
  let env = FunctionEnv::new(store, env);

  (
    wasmer::imports! {
      "dprint" => {
        "host_clear_bytes" => Function::new_typed_with_env(store, &env, host_clear_bytes),
        "host_read_buffer" => Function::new_typed_with_env(store, &env, host_read_buffer),
        "host_write_buffer" => Function::new_typed_with_env(store, &env, host_write_buffer),
        "host_take_override_config" => Function::new_typed_with_env(store, &env, host_take_override_config),
        "host_take_file_path" => Function::new_typed_with_env(store, &env, host_take_file_path),
        "host_format" => Function::new_typed_with_env(store, &env, host_format),
        "host_get_formatted_text" => Function::new_typed_with_env(store, &env, host_get_formatted_text),
        "host_get_error_text" => Function::new_typed_with_env(store, &env, host_get_error_text),
      }
    },
    Box::new(env),
  )
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
    self.ensure_config(config)?;
    match self.inner_format_text(file_path, file_bytes, override_config) {
      Ok(inner) => inner,
      Err(err) => Err(CriticalFormatError(err).into()),
    }
  }
}

struct WasmFunctions {
  store: Store,
  instance: WasmInstance,
  memory: Memory,
}

impl WasmFunctions {
  pub fn new(store: Store, instance: WasmInstance) -> Result<Self> {
    let memory = instance.get_memory("memory")?.clone();

    Ok(WasmFunctions { instance, memory, store })
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
    match self.instance.get_function(name) {
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
