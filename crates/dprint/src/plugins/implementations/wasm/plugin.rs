use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::PluginInfo;

use super::create_module;
use super::create_pools_import_object;
use super::load_instance;
use super::FormatResult;
use super::ImportObjectEnvironment;
use super::WasmFunctions;
use crate::configuration::RawPluginConfig;
use crate::environment::Environment;
use crate::plugins::InitializedPlugin;
use crate::plugins::Plugin;
use crate::plugins::PluginPools;

pub struct WasmPlugin<TEnvironment: Environment> {
  module: wasmer::Module,
  plugin_info: PluginInfo,
  config: Option<(RawPluginConfig, GlobalConfiguration)>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> WasmPlugin<TEnvironment> {
  pub fn new(compiled_wasm_bytes: Vec<u8>, plugin_info: PluginInfo, plugin_pools: Arc<PluginPools<TEnvironment>>) -> Result<Self> {
    let module = create_module(&compiled_wasm_bytes)?;
    Ok(WasmPlugin {
      module,
      plugin_info,
      config: None,
      plugin_pools,
    })
  }
}

impl<TEnvironment: Environment> Plugin for WasmPlugin<TEnvironment> {
  fn name(&self) -> &str {
    &self.plugin_info.name
  }

  fn version(&self) -> &str {
    &self.plugin_info.version
  }

  fn config_key(&self) -> &str {
    &self.plugin_info.config_key
  }

  fn file_extensions(&self) -> &Vec<String> {
    &self.plugin_info.file_extensions
  }

  fn file_names(&self) -> &Vec<String> {
    &self.plugin_info.file_names
  }

  fn help_url(&self) -> &str {
    &self.plugin_info.help_url
  }

  fn config_schema_url(&self) -> &str {
    &self.plugin_info.config_schema_url
  }

  fn set_config(&mut self, plugin_config: RawPluginConfig, global_config: GlobalConfiguration) {
    self.config = Some((plugin_config, global_config));
  }

  fn get_config(&self) -> &(RawPluginConfig, GlobalConfiguration) {
    self.config.as_ref().expect("Call set_config first.")
  }

  fn initialize(&self) -> Result<Box<dyn InitializedPlugin>> {
    let store = wasmer::Store::default();
    let mut wasm_plugin = InitializedWasmPlugin::new(
      self.module.clone(),
      Box::new({
        let name = self.name().to_string();
        let plugin_pools = self.plugin_pools.clone();
        move || {
          let import_obj_env = ImportObjectEnvironment::new(&name, plugin_pools.clone());
          create_pools_import_object(&store, &import_obj_env)
        }
      }),
    )?;
    let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config first.");

    wasm_plugin.set_global_config(&global_config)?;
    wasm_plugin.set_plugin_config(&plugin_config.properties)?;

    Ok(Box::new(wasm_plugin))
  }
}

pub struct InitializedWasmPlugin {
  wasm_functions: WasmFunctions,
  buffer_size: usize,

  // below is for recreating an instance after panic
  module: wasmer::Module,
  create_import_object: Box<dyn Fn() -> wasmer::ImportObject + Send>,
  global_config: GlobalConfiguration,
  plugin_config: ConfigKeyMap,
}

impl InitializedWasmPlugin {
  pub fn new(module: wasmer::Module, create_import_object: Box<dyn Fn() -> wasmer::ImportObject + Send>) -> Result<Self> {
    let instance = load_instance(&module, &create_import_object())?;
    let wasm_functions = WasmFunctions::new(instance)?;
    let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;

    Ok(InitializedWasmPlugin {
      wasm_functions,
      buffer_size,
      module,
      create_import_object,
      global_config: GlobalConfiguration {
        line_width: None,
        use_tabs: None,
        indent_width: None,
        new_line_kind: None,
      },
      plugin_config: HashMap::new(),
    })
  }

  pub fn set_global_config(&mut self, global_config: &GlobalConfiguration) -> Result<()> {
    let json = serde_json::to_string(global_config)?;
    self.send_string(&json);
    self.wasm_functions.set_global_config()?;
    self.global_config = global_config.clone();
    Ok(())
  }

  pub fn set_plugin_config(&mut self, plugin_config: &ConfigKeyMap) -> Result<()> {
    let json = serde_json::to_string(plugin_config)?;
    self.send_string(&json);
    self.wasm_functions.set_plugin_config()?;
    Ok(())
  }

  pub fn get_plugin_info(&self) -> Result<PluginInfo> {
    let len = self.wasm_functions.get_plugin_info()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  /* LOW LEVEL SENDING AND RECEIVING */

  // These methods should panic when failing because that may indicate
  // a major problem where the CLI is out of sync with the plugin.

  fn send_string(&self, text: &str) {
    let mut index = 0;
    let len = text.len();
    let text_bytes = text.as_bytes();
    self.wasm_functions.clear_shared_bytes(len).unwrap();
    while index < len {
      let write_count = std::cmp::min(len - index, self.buffer_size);
      self.write_bytes_to_memory_buffer(&text_bytes[index..(index + write_count)]);
      self.wasm_functions.add_to_shared_bytes_from_buffer(write_count).unwrap();
      index += write_count;
    }
  }

  fn write_bytes_to_memory_buffer(&self, bytes: &[u8]) {
    let length = bytes.len();
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr().unwrap();
    let memory_writer = wasm_buffer_pointer.deref(self.wasm_functions.get_memory(), 0, length as u32).unwrap();
    for i in 0..length {
      memory_writer[i].set(bytes[i]);
    }
  }

  fn receive_string(&self, len: usize) -> Result<String> {
    let mut index = 0;
    let mut bytes: Vec<u8> = vec![0; len];
    while index < len {
      let read_count = std::cmp::min(len - index, self.buffer_size);
      self.wasm_functions.set_buffer_with_shared_bytes(index, read_count).unwrap();
      self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)]);
      index += read_count;
    }
    Ok(String::from_utf8(bytes)?)
  }

  fn read_bytes_from_memory_buffer(&self, bytes: &mut [u8]) {
    let length = bytes.len();
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr().unwrap();
    let memory_reader = wasm_buffer_pointer.deref(self.wasm_functions.get_memory(), 0, length as u32).unwrap();
    for i in 0..length {
      bytes[i] = memory_reader[i].get();
    }
  }

  fn reinitialize_due_to_panic(&mut self, original_err: &Error) {
    if let Err(reinitialize_err) = self.try_reinitialize_due_to_panic() {
      panic!(
        "Originally panicked, then failed reinitialize. Cannot recover.\nOriginal error: {}\nReinitialize error: {}",
        original_err.to_string(),
        reinitialize_err.to_string(),
      )
    }
  }

  fn try_reinitialize_due_to_panic(&mut self) -> Result<()> {
    let instance = load_instance(&self.module, &(self.create_import_object)())?;
    let wasm_functions = WasmFunctions::new(instance)?;
    let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;

    self.wasm_functions = wasm_functions;
    self.buffer_size = buffer_size;

    self.set_global_config(&self.global_config.clone())?;
    self.set_plugin_config(&self.plugin_config.clone())?;

    Ok(())
  }
}

impl InitializedPlugin for InitializedWasmPlugin {
  fn get_license_text(&self) -> Result<String> {
    let len = self.wasm_functions.get_license_text()?;
    self.receive_string(len)
  }

  fn get_resolved_config(&self) -> Result<String> {
    let len = self.wasm_functions.get_resolved_config()?;
    self.receive_string(len)
  }

  fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>> {
    let len = self.wasm_functions.get_config_diagnostics()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn format_text(&mut self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> Result<String> {
    // send override config if necessary
    if !override_config.is_empty() {
      self.send_string(&serde_json::to_string(override_config)?);
      if let Err(err) = self.wasm_functions.set_override_config() {
        self.reinitialize_due_to_panic(&err);
        return Err(err);
      }
    }

    // send file path
    self.send_string(&file_path.to_string_lossy());

    if let Err(err) = self.wasm_functions.set_file_path() {
      self.reinitialize_due_to_panic(&err);
      return Err(err);
    }

    // send file text and format
    self.send_string(file_text);
    let response_code = match self.wasm_functions.format() {
      Ok(code) => code,
      Err(err) => {
        self.reinitialize_due_to_panic(&err);
        return Err(err);
      }
    };

    // handle the response
    match response_code {
      FormatResult::NoChange => Ok(String::from(file_text)),
      FormatResult::Change => {
        let len = match self.wasm_functions.get_formatted_text() {
          Ok(len) => len,
          Err(err) => {
            self.reinitialize_due_to_panic(&err);
            return Err(err);
          }
        };
        match self.receive_string(len) {
          Ok(text) => Ok(text),
          Err(err) => {
            self.reinitialize_due_to_panic(&err);
            return Err(err);
          }
        }
      }
      FormatResult::Error => {
        let len = match self.wasm_functions.get_error_text() {
          Ok(len) => len,
          Err(err) => {
            self.reinitialize_due_to_panic(&err);
            return Err(err);
          }
        };
        match self.receive_string(len) {
          Ok(text) => bail!("{}", text),
          Err(err) => {
            self.reinitialize_due_to_panic(&err);
            Err(err)
          }
        }
      }
    }
  }
}
