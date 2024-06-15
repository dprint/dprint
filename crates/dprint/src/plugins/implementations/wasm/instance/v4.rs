use std::collections::HashSet;
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
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use dprint_core::plugins::PluginInfo;
use parking_lot::Mutex;
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

use crate::plugins::implementations::wasm::ImportObjectEnvironment;
use crate::plugins::implementations::wasm::WasmHostFormatSender;
use crate::plugins::implementations::wasm::WasmInstance;
use crate::plugins::FormatConfig;

use super::InitializedWasmPluginInstance;

enum WasmFormatResult {
  NoChange,
  Change,
  Error,
}

pub fn create_identity_import_object(store: &mut Store) -> wasmer::Imports {
  let host_clear_bytes = |_: u32| {};
  let host_read_buffer = |_: u32, _: u32| {};
  let host_write_buffer = |_: u32| {};
  let host_take_override_config = || {};
  let host_take_file_path = || {};
  let host_format = || -> u32 { 0 }; // no change
  let host_get_formatted_text = || -> u32 { 0 }; // zero length
  let host_get_error_text = || -> u32 { 0 }; // zero length
  let host_has_cancelled = || -> u32 { 0 }; // false

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
      "host_has_cancelled" => Function::new_typed(store, host_has_cancelled),
    }
  }
}

pub fn create_pools_import_object(store: &mut Store, host_format_sender: WasmHostFormatSender) -> (wasmer::Imports, Box<dyn ImportObjectEnvironment>) {
  struct ImportObjectEnvironmentV4 {
    memory: Option<Memory>,
    override_config: Option<ConfigKeyMap>,
    file_path: Option<PathBuf>,
    formatted_text_store: Vec<u8>,
    shared_bytes: Mutex<Vec<u8>>,
    error_text_store: String,
    token: Arc<dyn CancellationToken>,
    host_format_sender: WasmHostFormatSender,
  }

  impl ImportObjectEnvironmentV4 {
    pub fn new(host_format_sender: WasmHostFormatSender) -> Self {
      ImportObjectEnvironmentV4 {
        memory: None,
        override_config: None,
        file_path: None,
        shared_bytes: Default::default(),
        formatted_text_store: Default::default(),
        error_text_store: Default::default(),
        token: Arc::new(NullCancellationToken),
        host_format_sender,
      }
    }

    fn take_shared_bytes(&self) -> Vec<u8> {
      let mut shared_bytes = self.shared_bytes.lock();
      std::mem::take(&mut shared_bytes)
    }
  }

  impl ImportObjectEnvironment for FunctionEnv<ImportObjectEnvironmentV4> {
    fn initialize(&self, store: &mut Store, instance: &Instance) -> Result<(), ExportError> {
      self.as_mut(store).memory = Some(instance.exports.get_memory("memory")?.clone());
      Ok(())
    }

    fn set_token(&self, store: &mut Store, token: Arc<dyn CancellationToken>) {
      self.as_mut(store).token = token;
    }
  }

  fn host_read_buffer(env: FunctionEnvMut<ImportObjectEnvironmentV4>, buffer_pointer: u32, length: u32) {
    let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);

    let length = length as usize;
    let mut shared_bytes = env_data.shared_bytes.lock();
    *shared_bytes = vec![0; length];
    memory_view.read(buffer_pointer.offset() as u64, &mut shared_bytes).unwrap();
  }

  fn host_write_buffer(env: FunctionEnvMut<ImportObjectEnvironmentV4>, buffer_pointer: u32) {
    let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);
    let shared_bytes = env_data.shared_bytes.lock();
    memory_view.write(buffer_pointer.offset() as u64, &shared_bytes).unwrap();
  }

  fn host_take_override_config(mut env: FunctionEnvMut<ImportObjectEnvironmentV4>) {
    let env = env.data_mut();
    let bytes = env.take_shared_bytes();
    let config_key_map: ConfigKeyMap = serde_json::from_slice(&bytes).unwrap_or_default();
    env.override_config.replace(config_key_map);
  }

  fn host_take_file_path(mut env: FunctionEnvMut<ImportObjectEnvironmentV4>) {
    let env = env.data_mut();
    let bytes = env.take_shared_bytes();
    let file_path_str = String::from_utf8(bytes).unwrap();
    env.file_path.replace(PathBuf::from(file_path_str));
  }

  fn host_format(mut env: FunctionEnvMut<ImportObjectEnvironmentV4>) -> u32 {
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

  fn host_get_formatted_text(mut env: FunctionEnvMut<ImportObjectEnvironmentV4>) -> u32 {
    let env = env.data_mut();
    let formatted_bytes = std::mem::take(&mut env.formatted_text_store);
    let len = formatted_bytes.len();
    *env.shared_bytes.lock() = formatted_bytes;
    len as u32
  }

  fn host_get_error_text(mut env: FunctionEnvMut<ImportObjectEnvironmentV4>) -> u32 {
    let env = env.data_mut();
    let error_text = std::mem::take(&mut env.error_text_store);
    let len = error_text.len();
    *env.shared_bytes.lock() = error_text.into_bytes();
    len as u32
  }

  fn host_has_cancelled(env: FunctionEnvMut<ImportObjectEnvironmentV4>) -> i32 {
    if env.data().token.as_ref().is_cancelled() {
      1
    } else {
      0
    }
  }

  let env = ImportObjectEnvironmentV4::new(host_format_sender);
  let env = FunctionEnv::new(store, env);

  (
    wasmer::imports! {
      "dprint" => {
        "host_read_buffer" => Function::new_typed_with_env(store, &env, host_read_buffer),
        "host_write_buffer" => Function::new_typed_with_env(store, &env, host_write_buffer),
        "host_take_override_config" => Function::new_typed_with_env(store, &env, host_take_override_config),
        "host_take_file_path" => Function::new_typed_with_env(store, &env, host_take_file_path),
        "host_format" => Function::new_typed_with_env(store, &env, host_format),
        "host_get_formatted_text" => Function::new_typed_with_env(store, &env, host_get_formatted_text),
        "host_get_error_text" => Function::new_typed_with_env(store, &env, host_get_error_text),
        "host_has_cancelled" => Function::new_typed_with_env(store, &env, host_has_cancelled),
      }
    },
    Box::new(env),
  )
}

pub struct InitializedWasmPluginInstanceV4 {
  wasm_functions: WasmFunctions,
  registered_config_ids: HashSet<FormatConfigId>,
}

impl InitializedWasmPluginInstanceV4 {
  pub fn new(store: Store, instance: WasmInstance) -> Result<Self> {
    let wasm_functions = WasmFunctions::new(store, instance)?;
    Ok(Self {
      wasm_functions,
      registered_config_ids: HashSet::new(),
    })
  }

  fn register_config(&mut self, config: &FormatConfig) -> Result<()> {
    #[derive(serde::Serialize)]
    struct RawFormatConfig<'a> {
      pub plugin: &'a ConfigKeyMap,
      pub global: &'a GlobalConfiguration,
    }

    let json = serde_json::to_string(&RawFormatConfig {
      plugin: &config.plugin,
      global: &config.global,
    })?;
    self.send_string(&json)?;
    self.wasm_functions.register_config(config.id)?;
    Ok(())
  }

  fn inner_format_text(&mut self, file_path: &Path, file_bytes: &[u8], config: &FormatConfig, override_config: &ConfigKeyMap) -> Result<FormatResult> {
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
    let response_code = self.wasm_functions.format(config.id)?;

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
    if !self.registered_config_ids.contains(&config.id) {
      // update the plugin
      self.register_config(config)?;
      // now mark this as successfully set
      self.registered_config_ids.insert(config.id);
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
    self.wasm_functions.clear_shared_bytes(bytes.len())?;
    self.write_bytes_to_shared_bytes(bytes)?;
    Ok(())
  }

  fn write_bytes_to_shared_bytes(&mut self, bytes: &[u8]) -> Result<()> {
    let shared_bytes_ptr = self.wasm_functions.get_shared_bytes_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.write(shared_bytes_ptr.offset() as u64, bytes)?;
    Ok(())
  }

  fn receive_string(&mut self, len: usize) -> Result<String> {
    let bytes = self.receive_bytes(len)?;
    Ok(String::from_utf8(bytes)?)
  }

  fn receive_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
    let mut bytes: Vec<u8> = vec![0; len];
    self.read_bytes_from_shared_bytes(&mut bytes)?;
    Ok(bytes)
  }

  fn read_bytes_from_shared_bytes(&mut self, bytes: &mut [u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_shared_bytes_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.read(wasm_buffer_pointer.offset() as u64, bytes)?;
    Ok(())
  }
}

impl InitializedWasmPluginInstance for InitializedWasmPluginInstanceV4 {
  fn plugin_info(&mut self) -> Result<PluginInfo> {
    let len = self.wasm_functions.get_plugin_info()?;
    let json_bytes = self.receive_bytes(len)?;
    Ok(serde_json::from_slice(&json_bytes)?)
  }

  fn license_text(&mut self) -> Result<String> {
    let len = self.wasm_functions.get_license_text()?;
    self.receive_string(len)
  }

  fn resolved_config(&mut self, config: &FormatConfig) -> Result<String> {
    self.ensure_config(config)?;
    let len = self.wasm_functions.get_resolved_config(config.id)?;
    self.receive_string(len)
  }

  fn config_diagnostics(&mut self, config: &FormatConfig) -> Result<Vec<ConfigurationDiagnostic>> {
    self.ensure_config(config)?;
    let len = self.wasm_functions.get_config_diagnostics(config.id)?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn file_matching_info(&mut self, config: &FormatConfig) -> Result<FileMatchingInfo> {
    self.ensure_config(config)?;
    let len = self.wasm_functions.get_config_file_matching(config.id)?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn format_text(
    &mut self,
    file_path: &Path,
    file_bytes: &[u8],
    config: &FormatConfig,
    override_config: &ConfigKeyMap,
    token: Arc<dyn CancellationToken>,
  ) -> FormatResult {
    self.wasm_functions.instance.set_token(&mut self.wasm_functions.store, token);
    self.ensure_config(config)?;
    match self.inner_format_text(file_path, file_bytes, config, override_config) {
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
  pub fn register_config(&mut self, config_id: FormatConfigId) -> Result<()> {
    let func = self.get_export::<u32, ()>("register_config")?;
    Ok(func.call(&mut self.store, config_id.as_raw())?)
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
  pub fn get_resolved_config(&mut self, config_id: FormatConfigId) -> Result<usize> {
    let get_resolved_config_func = self.get_export::<u32, u32>("get_resolved_config")?;
    Ok(get_resolved_config_func.call(&mut self.store, config_id.as_raw()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_config_diagnostics(&mut self, config_id: FormatConfigId) -> Result<usize> {
    let func = self.get_export::<u32, u32>("get_config_diagnostics")?;
    Ok(func.call(&mut self.store, config_id.as_raw()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_config_file_matching(&mut self, config_id: FormatConfigId) -> Result<usize> {
    let func = self.get_export::<u32, u32>("get_config_file_matching")?;
    Ok(func.call(&mut self.store, config_id.as_raw()).map(|value| value as usize)?)
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
  pub fn format(&mut self, config_id: FormatConfigId) -> Result<WasmFormatResult> {
    let format_func = self.get_export::<u32, u8>("format")?;
    Ok(format_func.call(&mut self.store, config_id.as_raw()).map(u8_to_format_result)?)
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
  pub fn get_shared_bytes_buffer_ptr(&mut self) -> Result<WasmPtr<u32>> {
    let func = self.get_export::<(), WasmPtr<u32>>("get_shared_bytes_buffer")?;
    Ok(func.call(&mut self.store)?)
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
