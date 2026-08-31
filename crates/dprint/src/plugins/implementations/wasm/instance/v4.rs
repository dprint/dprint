use std::collections::HashSet;
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
use dprint_core::plugins::wasm::JsonResponse;
use wasmtime::Caller;
use wasmtime::Engine;
use wasmtime::Memory;
use wasmtime::TypedFunc;
use wasmtime::WasmParams;
use wasmtime::WasmResults;

use crate::environment::Environment;
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

/// A callback that logs plugin stderr output. Kept as a boxed callback rather
/// than storing the whole `Environment` + plugin name in the host state, which
/// would make the store data generic over the environment type.
pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

/// The host state for a v4 plugin, stored in the wasmtime `Store` data.
pub struct ImportObjectEnvironmentV4 {
  pub memory: Option<Memory>,
  pub token: Arc<dyn CancellationToken>,
  log: LogFn,
  formatted_text_store: Vec<u8>,
  shared_bytes: Vec<u8>,
  error_text_store: String,
  host_format_sender: WasmHostFormatSender,
}

pub fn add_identity_imports(linker: &mut Linker) -> Result<()> {
  linker.func_wrap("env", "fd_write", |_: u32, _: u32, _: u32, _: u32| -> u32 { 0 })?; // ignore
  linker.func_wrap("dprint", "host_write_buffer", |_: u32| {})?;
  linker.func_wrap(
    "dprint",
    "host_format",
    |_: u32, _: u32, _: u32, _: u32, _: u32, _: u32, _: u32, _: u32| -> u32 { 0 },
  )?; // no change
  linker.func_wrap("dprint", "host_get_formatted_text", || -> u32 { 0 })?; // zero length
  linker.func_wrap("dprint", "host_get_error_text", || -> u32 { 0 })?; // zero length
  linker.func_wrap("dprint", "host_has_cancelled", || -> i32 { 0 })?; // false
  Ok(())
}

pub fn create_pools_import_object<TEnvironment: Environment>(
  environment: TEnvironment,
  plugin_name: String,
  engine: &Engine,
  host_format_sender: WasmHostFormatSender,
) -> Result<(Linker, WasmHostState)> {
  let log: LogFn = Arc::new(move |text: &str| environment.log_stderr_with_context(text, &plugin_name));
  let state = ImportObjectEnvironmentV4 {
    memory: None,
    token: Arc::new(NullCancellationToken),
    log,
    formatted_text_store: Default::default(),
    shared_bytes: Default::default(),
    error_text_store: Default::default(),
    host_format_sender,
  };
  let mut linker = Linker::new(engine);
  linker.func_wrap("env", "fd_write", fd_write)?;
  linker.func_wrap("dprint", "host_write_buffer", host_write_buffer)?;
  linker.func_wrap("dprint", "host_format", host_format)?;
  linker.func_wrap("dprint", "host_get_formatted_text", host_get_formatted_text)?;
  linker.func_wrap("dprint", "host_get_error_text", host_get_error_text)?;
  linker.func_wrap("dprint", "host_has_cancelled", host_has_cancelled)?;
  Ok((linker, WasmHostState::V4(state)))
}

fn env<'a>(caller: &'a Caller<'_, WasmHostState>) -> &'a ImportObjectEnvironmentV4 {
  match caller.data() {
    WasmHostState::V4(state) => state,
    _ => unreachable!("expected v4 host state"),
  }
}

fn env_mut<'a>(caller: &'a mut Caller<'_, WasmHostState>) -> &'a mut ImportObjectEnvironmentV4 {
  match caller.data_mut() {
    WasmHostState::V4(state) => state,
    _ => unreachable!("expected v4 host state"),
  }
}

fn fd_write(mut caller: Caller<'_, WasmHostState>, fd: u32, iovs_ptr: u32, iovs_len: u32, nwritten_ptr: u32) -> u32 {
  let memory = env(&caller).memory.unwrap();
  let log = env(&caller).log.clone();

  let mut total_written: u32 = 0;
  for i in 0..iovs_len {
    let iovec_offset = (iovs_ptr + i * 8) as usize;
    let mut iovec = [0u8; 8];
    if memory.read(&caller, iovec_offset, &mut iovec).is_err() {
      return 1;
    }
    let buf_addr = u32::from_le_bytes(iovec[0..4].try_into().unwrap());
    let buf_len = u32::from_le_bytes(iovec[4..8].try_into().unwrap());

    let mut bytes = vec![0u8; buf_len as usize];
    if memory.read(&caller, buf_addr as usize, &mut bytes).is_err() {
      return 1;
    }

    if matches!(fd, 1 | 2) {
      log(&String::from_utf8_lossy(&bytes));
    } else {
      return 1; // unsupported fd
    }

    total_written += buf_len;
  }

  if memory.write(&mut caller, nwritten_ptr as usize, &total_written.to_le_bytes()).is_err() {
    return 1;
  }

  0
}

fn host_write_buffer(mut caller: Caller<'_, WasmHostState>, buffer_pointer: u32) {
  let memory = env(&caller).memory.unwrap();
  let bytes = std::mem::take(&mut env_mut(&mut caller).shared_bytes);
  memory.write(&mut caller, buffer_pointer as usize, &bytes).unwrap();
  env_mut(&mut caller).shared_bytes = bytes;
}

#[allow(clippy::too_many_arguments)]
fn host_format(
  mut caller: Caller<'_, WasmHostState>,
  file_path_ptr: u32,
  file_path_len: u32,
  range_start: u32,
  range_end: u32,
  override_cfg_ptr: u32,
  override_cfg_len: u32,
  file_bytes_ptr: u32,
  file_bytes_len: u32,
) -> u32 {
  let memory = env(&caller).memory.unwrap();
  let override_config = if override_cfg_len == 0 {
    ConfigKeyMap::default()
  } else {
    let mut buf = vec![0u8; override_cfg_len as usize];
    memory.read(&caller, override_cfg_ptr as usize, &mut buf).unwrap();
    serde_json::from_slice::<ConfigKeyMap>(&buf).unwrap()
  };
  let file_path = {
    let mut buf = vec![0u8; file_path_len as usize];
    memory.read(&caller, file_path_ptr as usize, &mut buf).unwrap();
    PathBuf::from(String::from_utf8(buf).unwrap())
  };
  let file_bytes = {
    let mut buf = vec![0u8; file_bytes_len as usize];
    memory.read(&caller, file_bytes_ptr as usize, &mut buf).unwrap();
    buf
  };
  let range = if range_start == 0 && range_end == file_bytes_len {
    None
  } else {
    Some(range_start as usize..range_end as usize)
  };
  let (token, host_format_sender) = {
    let env = env(&caller);
    (env.token.clone(), env.host_format_sender.clone())
  };
  let request = HostFormatRequest {
    file_path,
    file_bytes,
    range,
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
  env.shared_bytes = formatted_bytes;
  len as u32
}

fn host_get_error_text(mut caller: Caller<'_, WasmHostState>) -> u32 {
  let env = env_mut(&mut caller);
  let error_text = std::mem::take(&mut env.error_text_store);
  let len = error_text.len();
  env.shared_bytes = error_text.into_bytes();
  len as u32
}

fn host_has_cancelled(caller: Caller<'_, WasmHostState>) -> i32 {
  if env(&caller).token.as_ref().is_cancelled() { 1 } else { 0 }
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
        Ok(Err(FormatError::new(text)))
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
    self.wasm_functions.write_memory(shared_bytes_ptr as usize, bytes)?;
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
    self.wasm_functions.read_memory(wasm_buffer_pointer as usize, bytes)?;
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
    let Some(len) = self.wasm_functions.check_config_updates()? else {
      return Ok(Vec::new());
    };
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
    self.ensure_config(config).map_err(FormatError::new)?;
    match self.inner_format_text(file_path, file_bytes, range, config, override_config.as_deref()) {
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
  pub fn register_config(&mut self, config_id: FormatConfigId) -> Result<()> {
    let func = self.get_export::<u32, ()>("register_config")?;
    Ok(func.call(&mut self.store, config_id.as_raw())?)
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
  pub fn check_config_updates(&mut self) -> Result<Option<usize>> {
    let maybe_func = self.get_maybe_export::<(), u32>("check_config_updates")?;
    match maybe_func {
      Some(func) => Ok(Some(func.call(&mut self.store, ()).map(|value| value as usize)?)),
      None => Ok(None), // ignore, the plugin doesn't have this defined
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
    let func = self.get_export::<(), ()>("set_override_config")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn set_file_path(&mut self) -> Result<()> {
    let func = self.get_export::<(), ()>("set_file_path")?;
    Ok(func.call(&mut self.store, ())?)
  }

  #[inline]
  pub fn format(&mut self, config_id: FormatConfigId) -> Result<WasmFormatResult> {
    let func = self.get_export::<u32, u32>("format")?;
    Ok(func.call(&mut self.store, config_id.as_raw()).map(|value| u8_to_format_result(value as u8))?)
  }

  #[inline]
  pub fn format_range(&mut self, config_id: FormatConfigId, range: std::ops::Range<usize>) -> Result<WasmFormatResult> {
    let maybe_func = self.get_maybe_export::<(u32, u32, u32), u32>("format_range")?;
    match maybe_func {
      Some(func) => Ok(
        func
          .call(&mut self.store, (config_id.as_raw(), range.start as u32, range.end as u32))
          .map(|value| u8_to_format_result(value as u8))?,
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
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn get_error_text(&mut self) -> Result<usize> {
    let func = self.get_export::<(), u32>("get_error_text")?;
    Ok(func.call(&mut self.store, ()).map(|value| value as usize)?)
  }

  #[inline]
  pub fn clear_shared_bytes(&mut self, capacity: usize) -> Result<u32> {
    let func = self.get_export::<u32, u32>("clear_shared_bytes")?;
    Ok(func.call(&mut self.store, capacity as u32)?)
  }

  #[inline]
  pub fn get_shared_bytes_ptr(&mut self) -> Result<u32> {
    let func = self.get_export::<(), u32>("get_shared_bytes_ptr")?;
    Ok(func.call(&mut self.store, ())?)
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
    match self.get_maybe_export(name)? {
      Some(export) => Ok(export),
      None => bail!("Could not find export '{}' in plugin.", name),
    }
  }

  fn get_maybe_export<P, R>(&mut self, name: &str) -> Result<Option<TypedFunc<P, R>>>
  where
    P: WasmParams,
    R: WasmResults,
  {
    match self.instance.get_function(&mut self.store, name) {
      Some(func) => match func.typed::<P, R>(&self.store) {
        Ok(typed_func) => Ok(Some(typed_func)),
        Err(err) => bail!("Error creating function '{}'. Message: {:#}", name, err),
      },
      None => Ok(None),
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
