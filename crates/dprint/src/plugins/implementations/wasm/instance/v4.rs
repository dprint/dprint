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
use dprint_core::plugins::wasm::JsonResponse;
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

use crate::environment::Environment;
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
  let host_write_buffer = |_: u32| {};
  let host_format = |_: u32, _: u32, _: u32, _: u32, _: u32, _: u32, _: u32, _: u32| -> u32 { 0 }; // no change
  let host_get_formatted_text = || -> u32 { 0 }; // zero length
  let host_get_error_text = || -> u32 { 0 }; // zero length
  let host_has_cancelled = || -> u32 { 0 }; // false
  let fd_write = |_: u32, _: u32, _: u32, _: u32| 0; // ignore

  wasmer::imports! {
    "env" => {
      "fd_write" => Function::new_typed(store, fd_write),
    },
    "dprint" => {
      "host_write_buffer" => Function::new_typed(store, host_write_buffer),
      "host_format" => Function::new_typed(store, host_format),
      "host_get_formatted_text" => Function::new_typed(store, host_get_formatted_text),
      "host_get_error_text" => Function::new_typed(store, host_get_error_text),
      "host_has_cancelled" => Function::new_typed(store, host_has_cancelled),
    }
  }
}

pub fn create_pools_import_object<TEnvironment: Environment>(
  environment: TEnvironment,
  plugin_name: String,
  store: &mut Store,
  host_format_sender: WasmHostFormatSender,
) -> (wasmer::Imports, Box<dyn ImportObjectEnvironment>) {
  struct ImportObjectEnvironmentV4<TEnvironment: Environment> {
    environment: TEnvironment,
    plugin_name: String,
    memory: Option<Memory>,
    formatted_text_store: Vec<u8>,
    shared_bytes: Mutex<Vec<u8>>,
    error_text_store: String,
    token: Arc<dyn CancellationToken>,
    host_format_sender: WasmHostFormatSender,
  }

  impl<TEnvironment: Environment> ImportObjectEnvironment for FunctionEnv<ImportObjectEnvironmentV4<TEnvironment>> {
    fn initialize(&self, store: &mut Store, instance: &Instance) -> Result<(), ExportError> {
      self.as_mut(store).memory = Some(instance.exports.get_memory("memory")?.clone());
      Ok(())
    }

    fn set_token(&self, store: &mut Store, token: Arc<dyn CancellationToken>) {
      self.as_mut(store).token = token;
    }
  }

  fn fd_write<TEnvironment: Environment>(
    env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>,
    fd: u32,
    iovs_ptr: u32,
    iovs_len: u32,
    nwritten: WasmPtr<u32>,
  ) -> u32 {
    #[derive(Copy, Clone)]
    #[repr(C)]
    struct Iovec {
      buf: u32,
      buf_len: u32,
    }

    unsafe impl wasmer::ValueType for Iovec {
      fn zero_padding_bytes(&self, _bytes: &mut [std::mem::MaybeUninit<u8>]) {}
    }

    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);

    let mut total_written = 0;

    for i in 0..iovs_len {
      let iovec_ptr = WasmPtr::<Iovec>::new(iovs_ptr + i * std::mem::size_of::<Iovec>() as u32);
      let Ok(iovec) = iovec_ptr.deref(&memory_view).read() else {
        return 1;
      };

      let buf_addr = iovec.buf;
      let buf_len = iovec.buf_len;

      let mut bytes = vec![0; buf_len as usize];
      let success = memory_view.read(buf_addr as u64, &mut bytes).is_ok();
      if !success {
        return 1;
      }

      if matches!(fd, 1 | 2) {
        let text = String::from_utf8_lossy(&bytes);
        env_data.environment.log_stderr_with_context(&text, &env_data.plugin_name);
      } else {
        return 1; // Indicate error for unsupported fd
      }

      total_written += buf_len;
    }

    let nwritten = nwritten.deref(&memory_view);
    let success = nwritten.write(total_written).is_ok();
    if !success {
      return 1;
    }

    0
  }

  fn host_write_buffer<TEnvironment: Environment>(env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>, buffer_pointer: u32) {
    let buffer_pointer: wasmer::WasmPtr<u32> = wasmer::WasmPtr::new(buffer_pointer);
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);
    let shared_bytes = env_data.shared_bytes.lock();
    memory_view.write(buffer_pointer.offset() as u64, &shared_bytes).unwrap();
  }

  #[allow(clippy::too_many_arguments)]
  fn host_format<TEnvironment: Environment>(
    mut env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>,
    file_path_ptr: u32,
    file_path_len: u32,
    range_start: u32,
    range_end: u32,
    override_cfg_ptr: u32,
    override_cfg_len: u32,
    file_bytes_ptr: u32,
    file_bytes_len: u32,
  ) -> u32 {
    let env_data = env.data();
    let memory = env_data.memory.as_ref().unwrap();
    let store_ref = env.as_store_ref();
    let memory_view = memory.view(&store_ref);
    let override_config = {
      if override_cfg_len == 0 {
        Default::default()
      } else {
        let mut buf = vec![0; override_cfg_len as usize];
        memory_view.read(override_cfg_ptr as u64, &mut buf).unwrap();
        serde_json::from_slice::<ConfigKeyMap>(&buf).unwrap()
      }
    };
    let file_path = {
      let mut buf = vec![0; file_path_len as usize];
      memory_view.read(file_path_ptr as u64, &mut buf).unwrap();
      PathBuf::from(String::from_utf8(buf).unwrap())
    };
    let file_bytes = {
      let mut buf = vec![0; file_bytes_len as usize];
      memory_view.read(file_bytes_ptr as u64, &mut buf).unwrap();
      buf
    };
    let range = if range_start == 0 && range_end == file_bytes_len {
      None
    } else {
      Some(range_start as usize..range_end as usize)
    };
    let env = env.data_mut();
    let request = HostFormatRequest {
      file_path,
      file_bytes,
      range,
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
          Ok(None) // receive error
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

  fn host_get_formatted_text<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>) -> u32 {
    let env = env.data_mut();
    let formatted_bytes = std::mem::take(&mut env.formatted_text_store);
    let len = formatted_bytes.len();
    *env.shared_bytes.lock() = formatted_bytes;
    len as u32
  }

  fn host_get_error_text<TEnvironment: Environment>(mut env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>) -> u32 {
    let env = env.data_mut();
    let error_text = std::mem::take(&mut env.error_text_store);
    let len = error_text.len();
    *env.shared_bytes.lock() = error_text.into_bytes();
    len as u32
  }

  fn host_has_cancelled<TEnvironment: Environment>(env: FunctionEnvMut<ImportObjectEnvironmentV4<TEnvironment>>) -> i32 {
    if env.data().token.as_ref().is_cancelled() {
      1
    } else {
      0
    }
  }

  let env = ImportObjectEnvironmentV4 {
    environment,
    plugin_name,
    memory: None,
    shared_bytes: Default::default(),
    formatted_text_store: Default::default(),
    error_text_store: Default::default(),
    token: Arc::new(NullCancellationToken),
    host_format_sender,
  };
  let env = FunctionEnv::new(store, env);

  (
    wasmer::imports! {
      "env" => {
        "fd_write" => Function::new_typed_with_env(store, &env, fd_write),
      },
      "dprint" => {
        "host_write_buffer" => Function::new_typed_with_env(store, &env, host_write_buffer),
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

  fn inner_format_text(
    &mut self,
    file_path: &Path,
    file_bytes: &[u8],
    range: FormatRange,
    config: &FormatConfig,
    override_config: Option<&str>,
  ) -> Result<FormatResult> {
    self.inner_setup_formatting(file_path, file_bytes, override_config)?;
    let response_code = match range {
      Some(range) => self.wasm_functions.format_range(config.id, range)?,
      None => self.wasm_functions.format(config.id)?,
    };
    self.inner_handle_response(response_code)
  }

  fn inner_setup_formatting(&mut self, file_path: &Path, file_bytes: &[u8], override_config: Option<&str>) -> Result<()> {
    // send override config if necessary
    if let Some(override_config) = override_config {
      self.send_string(override_config)?;
      self.wasm_functions.set_override_config()?;
    }

    // send file path
    self.send_string(&file_path.to_string_lossy())?;
    self.wasm_functions.set_file_path()?;

    // send file text
    self.send_bytes(file_bytes)
  }

  fn inner_handle_response(&mut self, response_code: WasmFormatResult) -> Result<FormatResult> {
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
    let shared_bytes_ptr = self.wasm_functions.clear_shared_bytes(bytes.len())?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.write(shared_bytes_ptr.offset() as u64, bytes)?;
    Ok(())
  }

  fn receive_string(&mut self, len: usize) -> Result<String> {
    let bytes = self.receive_bytes(len)?;
    Ok(String::from_utf8(bytes)?)
  }

  fn receive_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len)?;
    bytes.resize(len, 0);
    self.read_bytes_from_shared_bytes(&mut bytes)?;
    Ok(bytes)
  }

  fn read_bytes_from_shared_bytes(&mut self, bytes: &mut [u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_shared_bytes_ptr()?;
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

  fn check_config_updates(&mut self, message: &CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    let bytes = serde_json::to_vec(&message)?;
    self.send_bytes(&bytes)?;
    let len = self.wasm_functions.check_config_updates()?;
    let bytes = self.receive_bytes(len)?;
    let result: JsonResponse = serde_json::from_slice(&bytes)?;
    match result {
      JsonResponse::Ok(value) => Ok(serde_json::from_value(value)?),
      JsonResponse::Err(err) => Err(anyhow!("{}", err)),
    }
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
    range: FormatRange,
    config: &FormatConfig,
    override_config: &ConfigKeyMap,
    token: Arc<dyn CancellationToken>,
  ) -> FormatResult {
    let override_config = if !override_config.is_empty() {
      Some(serde_json::to_string(override_config)?)
    } else {
      None
    };
    self.wasm_functions.instance.set_token(&mut self.wasm_functions.store, token);
    self.ensure_config(config)?;
    match self.inner_format_text(file_path, file_bytes, range, config, override_config.as_deref()) {
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
    let func = self.get_export::<(), u32>("get_plugin_info")?;
    Ok(func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_license_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_license_text")?;
    Ok(func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn check_config_updates(&mut self) -> Result<usize> {
    let maybe_func = self.get_maybe_export::<(), u32>("check_config_updates")?;
    match maybe_func {
      Some(func) => Ok(func.call(&mut self.store).map(|value| value as usize)?),
      None => Ok(0), // ignore, the plugin doesn't have this defined
    }
  }

  #[inline]
  pub fn get_resolved_config(&mut self, config_id: FormatConfigId) -> Result<usize> {
    let func = self.get_export::<u32, u32>("get_resolved_config")?;
    Ok(func.call(&mut self.store, config_id.as_raw()).map(|value| value as usize)?)
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
    let func = self.get_export::<(), ()>("set_file_path")?;
    Ok(func.call(&mut self.store)?)
  }

  #[inline]
  pub fn format(&mut self, config_id: FormatConfigId) -> Result<WasmFormatResult> {
    let func = self.get_export::<u32, u8>("format")?;
    Ok(func.call(&mut self.store, config_id.as_raw()).map(u8_to_format_result)?)
  }

  #[inline]
  pub fn format_range(&mut self, config_id: FormatConfigId, range: std::ops::Range<usize>) -> Result<WasmFormatResult> {
    let maybe_func = self.get_maybe_export::<(u32, u32, u32), u8>("format_range")?;
    match maybe_func {
      Some(func) => Ok(
        func
          .call(&mut self.store, config_id.as_raw(), range.start as u32, range.end as u32)
          .map(u8_to_format_result)?,
      ),
      None => {
        // not supported
        Ok(WasmFormatResult::NoChange)
      }
    }
  }

  #[inline]
  pub fn get_formatted_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_formatted_text")?;
    Ok(func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_error_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_error_text")?;
    Ok(func.call(&mut self.store).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_memory_view(&self) -> MemoryView {
    self.memory.view(&self.store)
  }

  #[inline]
  pub fn clear_shared_bytes(&mut self, capacity: usize) -> Result<WasmPtr<u32>> {
    let clear_shared_bytes_func = self.get_export::<u32, WasmPtr<u32>>("clear_shared_bytes")?;
    Ok(clear_shared_bytes_func.call(&mut self.store, capacity as u32)?)
  }

  #[inline]
  pub fn get_shared_bytes_ptr(&mut self) -> Result<WasmPtr<u32>> {
    let func = self.get_export::<(), WasmPtr<u32>>("get_shared_bytes_ptr")?;
    Ok(func.call(&mut self.store)?)
  }

  fn get_export<Args, Rets>(&mut self, name: &str) -> Result<TypedFunction<Args, Rets>>
  where
    Args: WasmTypeList,
    Rets: WasmTypeList,
  {
    let maybe_export = self.get_maybe_export(name)?;
    match maybe_export {
      Some(export) => Ok(export),
      None => bail!("Could not find export '{}' in plugin.", name),
    }
  }

  fn get_maybe_export<Args, Rets>(&mut self, name: &str) -> Result<Option<TypedFunction<Args, Rets>>>
  where
    Args: WasmTypeList,
    Rets: WasmTypeList,
  {
    match self.instance.get_function(name) {
      Ok(func) => match func.typed::<Args, Rets>(&self.store) {
        Ok(native_func) => Ok(Some(native_func)),
        Err(err) => bail!("Error creating function '{}'. Message: {:#}", name, err),
      },
      Err(err) => match err {
        ExportError::IncompatibleType => {
          bail!("Export '{}'. {:#}", name, err)
        }
        ExportError::Missing(_) => Ok(None),
      },
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
