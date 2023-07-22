use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::wasm;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use futures::FutureExt;
use std::cell::RefCell;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::PluginInfo;

use super::create_pools_import_object;
use super::load_instance;
use super::WasmFormatResult;
use super::WasmFunctions;
use super::WasmHostFormatCallback;
use super::WasmHostFormatCell;
use super::WasmModuleCreator;
use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;

pub struct WasmPlugin<TEnvironment: Environment> {
  module: wasmer::Module,
  environment: TEnvironment,
  plugin_info: PluginInfo,
}

impl<TEnvironment: Environment> WasmPlugin<TEnvironment> {
  pub fn new(compiled_wasm_bytes: &[u8], plugin_info: PluginInfo, wasm_module_creator: &WasmModuleCreator, environment: TEnvironment) -> Result<Self> {
    let module = wasm_module_creator.create_from_serialized(compiled_wasm_bytes)?;
    Ok(WasmPlugin {
      module,
      environment,
      plugin_info,
    })
  }
}

impl<TEnvironment: Environment> Plugin for WasmPlugin<TEnvironment> {
  fn info(&self) -> &PluginInfo {
    &self.plugin_info
  }

  fn is_process_plugin(&self) -> bool {
    false
  }

  fn initialize(&self) -> LocalBoxFuture<'static, Result<Rc<dyn InitializedPlugin>>> {
    let plugin = InitializedWasmPlugin::new(
      self.info().name.to_string(),
      self.module.clone(),
      Arc::new({
        let environment = self.environment.clone();
        move |store, module, host_format_callback| {
          let (import_object, env) = create_pools_import_object(environment.clone(), store, host_format_callback);
          let instance = load_instance(store, module, &import_object)?;
          env.as_mut(store).initialize(&instance)?;
          Ok(instance)
        }
      }),
      self.environment.clone(),
    );
    async move {
      let result: Rc<dyn InitializedPlugin> = Rc::new(plugin);
      Ok(result)
    }
    .boxed_local()
  }
}

struct InitializedWasmPluginInstanceWithHostFormatCell {
  instance: InitializedWasmPluginInstance,
  host_format_cell: WasmHostFormatCell,
}

pub(super) struct InitializedWasmPluginInstance {
  wasm_functions: WasmFunctions,
  buffer_size: usize,
  current_config_id: FormatConfigId,
}

impl InitializedWasmPluginInstance {
  pub fn new(mut wasm_functions: WasmFunctions) -> Result<Self> {
    let buffer_size = wasm_functions.get_wasm_memory_buffer_size()?;
    Ok(Self {
      wasm_functions,
      buffer_size,
      current_config_id: FormatConfigId::uninitialized(),
    })
  }

  pub fn set_global_config(&mut self, global_config: &GlobalConfiguration) -> Result<()> {
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

  pub fn plugin_info(&mut self) -> Result<PluginInfo> {
    let len = self.wasm_functions.get_plugin_info()?;
    let json_text = self.receive_string(len)?;
    Ok(serde_json::from_str(&json_text)?)
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

  fn format_text(&mut self, file_path: &Path, file_text: &str, config: &FormatConfig, override_config: &ConfigKeyMap) -> FormatResult {
    self.ensure_config(config)?;
    match self.inner_format_text(file_path, file_text, override_config) {
      Ok(inner) => inner,
      Err(err) => Err(CriticalFormatError(err).into()),
    }
  }

  fn inner_format_text(&mut self, file_path: &Path, file_text: &str, override_config: &ConfigKeyMap) -> Result<FormatResult> {
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

  fn write_bytes_to_memory_buffer(&mut self, bytes: &[u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.write(wasm_buffer_pointer.offset() as u64, bytes)?;
    Ok(())
  }

  fn receive_string(&mut self, len: usize) -> Result<String> {
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

  fn read_bytes_from_memory_buffer(&mut self, bytes: &mut [u8]) -> Result<()> {
    let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr()?;
    let memory_view = self.wasm_functions.get_memory_view();
    memory_view.read(wasm_buffer_pointer.offset() as u64, bytes)?;
    Ok(())
  }
}

type LoadInstanceFn = dyn Fn(&mut wasmer::Store, &wasmer::Module, WasmHostFormatCallback) -> Result<wasmer::Instance> + Send + Sync;

struct InitializedWasmPluginInner<TEnvironment: Environment> {
  name: String,
  instances: RefCell<Vec<InitializedWasmPluginInstanceWithHostFormatCell>>,
  module: wasmer::Module,
  load_instance: Arc<LoadInstanceFn>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> Drop for InitializedWasmPluginInner<TEnvironment> {
  fn drop(&mut self) {
    let start = Instant::now();
    let len = {
      let mut instances = self.instances.borrow_mut();
      let result = std::mem::take(&mut *instances);
      result.len()
    };
    log_verbose!(
      self.environment,
      "Dropped {} ({} instances) in {}ms",
      self.name,
      len,
      start.elapsed().as_millis()
    );
  }
}

pub struct InitializedWasmPlugin<TEnvironment: Environment>(InitializedWasmPluginInner<TEnvironment>);

impl<TEnvironment: Environment> InitializedWasmPlugin<TEnvironment> {
  pub fn new(name: String, module: wasmer::Module, load_instance: Arc<LoadInstanceFn>, environment: TEnvironment) -> Self {
    Self(InitializedWasmPluginInner {
      name,
      instances: Default::default(),
      module,
      load_instance,
      environment,
    })
  }

  async fn with_instance<T: Send + Sync + 'static>(
    &self,
    host_format_callback: Option<HostFormatCallback>,
    action: impl Fn(&mut InitializedWasmPluginInstance) -> Result<T> + Send + Sync + 'static,
  ) -> Result<T> {
    let plugin = match self.get_or_create_instance(host_format_callback.clone()).await {
      Ok(instance) => instance,
      Err(err) => return Err(CriticalFormatError(err).into()),
    };
    let host_format_cell = plugin.host_format_cell;
    let mut instance = plugin.instance;
    let (result, action, instance) = dprint_core::async_runtime::spawn_blocking(move || {
      let result = action(&mut instance);
      (result, action, instance)
    })
    .await?;
    match result {
      Ok(result) => {
        self.release_instance(InitializedWasmPluginInstanceWithHostFormatCell { instance, host_format_cell });
        Ok(result)
      }
      Err(original_err) if original_err.downcast_ref::<CriticalFormatError>().is_some() => {
        let plugin = match self.get_or_create_instance(host_format_callback).await {
          Ok(plugin) => plugin,
          Err(err) => return Err(CriticalFormatError(err).into()),
        };
        let host_format_cell = plugin.host_format_cell;
        let mut instance = plugin.instance;

        // try again
        let (result, instance) = dprint_core::async_runtime::spawn_blocking(move || {
          let result = action(&mut instance);
          (result, instance)
        })
        .await?;
        match result {
          Ok(result) => {
            self.release_instance(InitializedWasmPluginInstanceWithHostFormatCell { instance, host_format_cell });
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
            self.release_instance(InitializedWasmPluginInstanceWithHostFormatCell { instance, host_format_cell });
            Err(err)
          }
        }
      }
      Err(err) => {
        self.release_instance(InitializedWasmPluginInstanceWithHostFormatCell { instance, host_format_cell });
        Err(err)
      }
    }
  }

  async fn get_or_create_instance(&self, host_format_callback: Option<HostFormatCallback>) -> Result<InitializedWasmPluginInstanceWithHostFormatCell> {
    let plugin = match self.0.instances.borrow_mut().pop() {
      Some(instance) => instance,
      None => self.create_instance().await?,
    };
    plugin.host_format_cell.set(host_format_callback);
    Ok(plugin)
  }

  fn release_instance(&self, plugin: InitializedWasmPluginInstanceWithHostFormatCell) {
    plugin.host_format_cell.clear();
    self.0.instances.borrow_mut().push(plugin);
  }

  async fn create_instance(&self) -> Result<InitializedWasmPluginInstanceWithHostFormatCell> {
    let start_instant = Instant::now();
    log_verbose!(self.0.environment, "Creating instance of {}", self.0.name);
    let mut store = wasmer::Store::default();
    let environment = self.0.environment.clone();
    let host_format_cell = WasmHostFormatCell::new();

    struct SendWrapper(WasmHostFormatCell);
    unsafe impl Send for SendWrapper {}
    unsafe impl Sync for SendWrapper {}

    let host_format_func: WasmHostFormatCallback = {
      let environment = environment.clone();
      let host_format_cell = SendWrapper(host_format_cell.clone());
      Box::new(move |req| {
        dprint_core::async_runtime::spawn_block_on_with_handle(
          environment.runtime_handle(),
          async move {
            let host_format = host_format_cell.0.get();
            debug_assert!(host_format.is_some(), "Expected host format callback to be set.");
            if host_format.is_none() {
              log_verbose!(environment, "WARNING: Host format callback was not set.");
            }
            match host_format {
              Some(host_format) => (host_format)(req).await,
              None => Ok(None),
            }
          }
          .boxed_local(),
        )?
      })
    };

    let load_instance = self.0.load_instance.clone();
    let module = self.0.module.clone();
    let (instance, store) = tokio::task::spawn_blocking(move || ((load_instance)(&mut store, &module, host_format_func), store)).await?;
    let wasm_functions = WasmFunctions::new(store, instance?)?;
    let instance = InitializedWasmPluginInstance::new(wasm_functions)?;
    log_verbose!(
      self.0.environment,
      "Created instance of {} in {}ms",
      self.0.name,
      start_instant.elapsed().as_millis() as u64
    );
    Ok(InitializedWasmPluginInstanceWithHostFormatCell { instance, host_format_cell })
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> InitializedPlugin for InitializedWasmPlugin<TEnvironment> {
  async fn license_text(&self) -> Result<String> {
    self.with_instance(None, move |instance| instance.license_text()).await
  }

  async fn resolved_config(&self, config: Arc<FormatConfig>) -> Result<String> {
    self.with_instance(None, move |instance| instance.resolved_config(&config)).await
  }

  async fn config_diagnostics(&self, config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>> {
    self.with_instance(None, move |instance| instance.config_diagnostics(&config)).await
  }

  async fn format_text(&self, request: InitializedPluginFormatRequest) -> FormatResult {
    // Wasm plugins do not currently support range formatting
    // so always return back None for now.
    if request.range.is_some() {
      return Ok(None);
    }
    if request.token.is_cancelled() {
      return Ok(None);
    }

    // todo: support cancellation in Wasm plugins
    let file_path = request.file_path;
    let file_text = request.file_text;
    let config = request.config;
    let override_config = request.override_config;
    self
      .with_instance(Some(request.on_host_format), move |instance| {
        instance.format_text(&file_path, &file_text, &config, &override_config)
      })
      .await
  }

  async fn shutdown(&self) {
    // do nothing
  }
}
