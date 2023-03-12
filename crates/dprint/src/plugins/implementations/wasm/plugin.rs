use anyhow::anyhow;
use anyhow::Result;
use dprint_core::plugins::BoxFuture;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FormatResult;
use futures::FutureExt;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::PluginInfo;

use super::create_module;
use super::create_pools_import_object;
use super::load_instance;
use super::CompiledWasmModuleBytes;
use super::ImportObjectEnvironment;
use super::WasmFormatResult;
use super::WasmFunctions;
use crate::configuration::RawPluginConfig;
use crate::environment::Environment;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;
use crate::plugins::PluginsCollection;

pub struct WasmPlugin<TEnvironment: Environment> {
  module: wasmer::Module,
  plugin_info: PluginInfo,
  config: Option<(RawPluginConfig, GlobalConfiguration)>,
  environment: TEnvironment,
  plugin_pools: Arc<PluginsCollection<TEnvironment>>,
}

impl<TEnvironment: Environment> WasmPlugin<TEnvironment> {
  pub fn new(
    compiled_wasm_bytes: &CompiledWasmModuleBytes,
    plugin_info: PluginInfo,
    environment: TEnvironment,
    plugin_pools: Arc<PluginsCollection<TEnvironment>>,
  ) -> Result<Self> {
    let module = create_module(compiled_wasm_bytes)?;
    Ok(WasmPlugin {
      environment,
      plugin_pools,
      module,
      plugin_info,
      config: None,
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

  fn update_url(&self) -> Option<&str> {
    self.plugin_info.update_url.as_deref()
  }

  fn set_config(&mut self, plugin_config: RawPluginConfig, global_config: GlobalConfiguration) {
    self.config = Some((plugin_config, global_config));
  }

  fn get_config(&self) -> &(RawPluginConfig, GlobalConfiguration) {
    self.config.as_ref().expect("Call set_config first.")
  }

  fn is_process_plugin(&self) -> bool {
    false
  }

  fn initialize(&self) -> BoxFuture<'static, Result<Arc<dyn InitializedPlugin>>> {
    let store = wasmer::Store::default();
    // need to call set_config first to ensure this doesn't fail
    let (plugin_config, global_config) = self.config.as_ref().unwrap();
    let plugin = InitializedWasmPlugin::new(
      self.name().to_string(),
      self.module.clone(),
      Arc::new({
        let plugin_pools = self.plugin_pools.clone();
        let environment = self.environment.clone();
        move || {
          let import_obj_env = ImportObjectEnvironment::new(environment.clone(), plugin_pools.clone());
          create_pools_import_object(&store, &import_obj_env)
        }
      }),
      global_config.clone(),
      plugin_config.properties.clone(),
    );
    async move {
      let result: Arc<dyn InitializedPlugin> = Arc::new(plugin);
      Ok(result)
    }
    .boxed()
  }
}

struct InitializedWasmPluginInstance {
  wasm_functions: WasmFunctions,
  buffer_size: usize,
}

impl InitializedWasmPluginInstance {
  pub fn set_global_config(&self, global_config: &GlobalConfiguration) -> Result<()> {
    let json = serde_json::to_string(global_config)?;
    self.send_string(&json)?;
    self.wasm_functions.set_global_config()?;
    Ok(())
  }

  pub fn set_plugin_config(&mut self, plugin_config: &ConfigKeyMap) -> Result<()> {
    let json = serde_json::to_string(plugin_config)?;
    self.send_string(&json)?;
    self.wasm_functions.set_plugin_config()?;
    Ok(())
  }

  pub fn plugin_info(&self) -> Result<PluginInfo> {
    let len = self.wasm_functions.get_plugin_info()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn license_text(&self) -> Result<String> {
    let len = self.wasm_functions.get_license_text()?;
    self.receive_string(len)
  }

  fn resolved_config(&self) -> Result<String> {
    let len = self.wasm_functions.get_resolved_config()?;
    self.receive_string(len)
  }

  fn config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>> {
    let len = self.wasm_functions.get_config_diagnostics()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
  }

  fn format_text(&self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> FormatResult {
    match self.inner_format_text(file_path, file_text, override_config) {
      Ok(inner) => inner,
      Err(err) => Err(CriticalFormatError(err).into()),
    }
  }

  fn inner_format_text(&self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> Result<FormatResult> {
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
    self.send_string(file_text)?;
    let response_code = self.wasm_functions.format()?;

    // handle the response
    match response_code {
      WasmFormatResult::NoChange => Ok(Ok(None)),
      WasmFormatResult::Change => {
        let len = self.wasm_functions.get_formatted_text()?;
        let text = self.receive_string(len)?;
        Ok(Ok(Some(text)))
      }
      WasmFormatResult::Error => {
        let len = self.wasm_functions.get_error_text()?;
        let text = self.receive_string(len)?;
        Ok(Err(anyhow!("{}", text)))
      }
    }
  }

  /* LOW LEVEL SENDING AND RECEIVING */

  // These methods should panic when failing because that may indicate
  // a major problem where the CLI is out of sync with the plugin.

  fn send_string(&self, text: &str) -> Result<()> {
    let mut index = 0;
    let len = text.len();
    let text_bytes = text.as_bytes();
    self.wasm_functions.clear_shared_bytes(len)?;
    while index < len {
      let write_count = std::cmp::min(len - index, self.buffer_size);
      self.write_bytes_to_memory_buffer(&text_bytes[index..(index + write_count)])?;
      self.wasm_functions.add_to_shared_bytes_from_buffer(write_count)?;
      index += write_count;
    }
    Ok(())
  }

  fn write_bytes_to_memory_buffer(&self, bytes: &[u8]) -> Result<()> {
    let length = bytes.len();
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_writer = wasm_buffer_pointer.deref(self.wasm_functions.get_memory(), 0, length as u32).unwrap();
    for i in 0..length {
      memory_writer[i].set(bytes[i]);
    }
    Ok(())
  }

  fn receive_string(&self, len: usize) -> Result<String> {
    let mut index = 0;
    let mut bytes: Vec<u8> = vec![0; len];
    while index < len {
      let read_count = std::cmp::min(len - index, self.buffer_size);
      self.wasm_functions.set_buffer_with_shared_bytes(index, read_count)?;
      self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)])?;
      index += read_count;
    }
    Ok(String::from_utf8(bytes)?)
  }

  fn read_bytes_from_memory_buffer(&self, bytes: &mut [u8]) -> Result<()> {
    let length = bytes.len();
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_reader = wasm_buffer_pointer.deref(self.wasm_functions.get_memory(), 0, length as u32).unwrap();
    for i in 0..length {
      bytes[i] = memory_reader[i].get();
    }
    Ok(())
  }
}

struct InitializedWasmPluginInner {
  name: String,
  instances: Mutex<Vec<InitializedWasmPluginInstance>>,
  // below is for recreating an instance after panic
  module: wasmer::Module,
  create_import_object: Arc<dyn Fn() -> wasmer::ImportObject + Send + Sync>,
  global_config: GlobalConfiguration,
  plugin_config: ConfigKeyMap,
}

#[derive(Clone)]
pub struct InitializedWasmPlugin(Arc<InitializedWasmPluginInner>);

impl InitializedWasmPlugin {
  pub fn new(
    name: String,
    module: wasmer::Module,
    create_import_object: Arc<dyn Fn() -> wasmer::ImportObject + Send + Sync>,
    global_config: GlobalConfiguration,
    plugin_config: ConfigKeyMap,
  ) -> Self {
    Self(Arc::new(InitializedWasmPluginInner {
      name,
      instances: Default::default(),
      module,
      create_import_object,
      global_config,
      plugin_config,
    }))
  }

  pub fn get_plugin_info(&self) -> Result<PluginInfo> {
    self.with_instance(|instance| instance.plugin_info())
  }

  fn with_instance<T>(&self, action: impl Fn(&InitializedWasmPluginInstance) -> Result<T>) -> Result<T> {
    let instance = match self.get_or_create_instance() {
      Ok(instance) => instance,
      Err(err) => return Err(CriticalFormatError(err).into()),
    };
    match action(&instance) {
      Ok(result) => {
        self.release_instance(instance);
        Ok(result)
      }
      Err(original_err) if original_err.downcast_ref::<CriticalFormatError>().is_some() => {
        let instance = match self.get_or_create_instance() {
          Ok(instance) => instance,
          Err(err) => return Err(CriticalFormatError(err).into()),
        };

        // try again
        match action(&instance) {
          Ok(result) => {
            self.release_instance(instance);
            Ok(result)
          }
          Err(reinitialize_err) if original_err.downcast_ref::<CriticalFormatError>().is_some() => Err(
            CriticalFormatError(anyhow!(
              concat!(
                "Originally panicked in {}, then failed reinitialize. ",
                "This may be a bug in the plugin, the dprint cli is out of date, or the ",
                "plugin is out of date.\nOriginal error: {}\nReinitialize error: {}",
              ),
              self.0.name,
              original_err,
              reinitialize_err,
            ))
            .into(),
          ),
          Err(err) => {
            self.release_instance(instance);
            Err(err)
          }
        }
      }
      Err(err) => {
        self.release_instance(instance);
        Err(err)
      }
    }
  }

  fn get_or_create_instance(&self) -> Result<InitializedWasmPluginInstance> {
    match self.0.instances.lock().pop() {
      Some(instance) => Ok(instance),
      None => self.create_instance(),
    }
  }

  fn release_instance(&self, plugin: InitializedWasmPluginInstance) {
    self.0.instances.lock().push(plugin);
  }

  fn create_instance(&self) -> Result<InitializedWasmPluginInstance> {
    let instance = load_instance(&self.0.module, &(self.0.create_import_object)())?;
    let wasm_functions = WasmFunctions::new(instance)?;
    let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;

    let mut instance = InitializedWasmPluginInstance { wasm_functions, buffer_size };

    instance.set_global_config(&self.0.global_config)?;
    instance.set_plugin_config(&self.0.plugin_config)?;

    Ok(instance)
  }
}

impl InitializedPlugin for InitializedWasmPlugin {
  fn license_text(&self) -> BoxFuture<'static, Result<String>> {
    let plugin = self.clone();
    async move {
      tokio::task::spawn_blocking(move || plugin.with_instance(move |instance| instance.license_text()))
        .await
        .unwrap()
    }
    .boxed()
  }

  fn resolved_config(&self) -> BoxFuture<'static, Result<String>> {
    let plugin = self.clone();
    async move {
      tokio::task::spawn_blocking(move || plugin.with_instance(move |instance| instance.resolved_config()))
        .await
        .unwrap()
    }
    .boxed()
  }

  fn config_diagnostics(&self) -> BoxFuture<'static, Result<Vec<ConfigurationDiagnostic>>> {
    let plugin = self.clone();
    async move {
      tokio::task::spawn_blocking(move || plugin.with_instance(move |instance| instance.config_diagnostics()))
        .await
        .unwrap()
    }
    .boxed()
  }

  fn format_text(&self, request: InitializedPluginFormatRequest) -> BoxFuture<'static, FormatResult> {
    let plugin = self.clone();
    async move {
      let file_path = request.file_path;
      let file_text = request.file_text;
      let override_config = request.override_config;
      // Wasm plugins do not currently support range formatting
      // so always return back None for now.
      if request.range.is_some() {
        return Ok(None);
      }
      if request.token.is_cancelled() {
        return Ok(None);
      }

      // todo: support cancellation in Wasm plugins
      tokio::task::spawn_blocking(move || plugin.with_instance(move |instance| instance.format_text(&file_path, &file_text, &override_config))).await?
    }
    .boxed()
  }

  fn shutdown(&self) -> BoxFuture<'static, ()> {
    Box::pin(futures::future::ready(()))
  }
}
