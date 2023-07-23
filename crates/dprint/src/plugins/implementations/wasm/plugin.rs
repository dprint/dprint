use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::JoinHandle;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::communication::Poisoner;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::PluginInfo;

use super::create_pools_import_object;
use super::load_instance;
use super::load_instance::WasmInstance;
use super::load_instance::WasmModule;
use super::WasmFormatResult;
use super::WasmFunctions;
use super::WasmHostFormatSender;
use super::WasmModuleCreator;
use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;

pub struct WasmPlugin<TEnvironment: Environment> {
  module: WasmModule,
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

#[async_trait(?Send)]
impl<TEnvironment: Environment> Plugin for WasmPlugin<TEnvironment> {
  fn info(&self) -> &PluginInfo {
    &self.plugin_info
  }

  fn is_process_plugin(&self) -> bool {
    false
  }

  async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>> {
    let plugin: Rc<dyn InitializedPlugin> = Rc::new(InitializedWasmPlugin::new(
      self.info().name.to_string(),
      self.module.clone(),
      Arc::new({
        move |store, module, host_format_callback| {
          let (import_object, env) = create_pools_import_object(store, host_format_callback);
          let instance = load_instance(store, module, &import_object)?;
          env.as_mut(store).initialize(&instance.inner)?;
          Ok(instance)
        }
      }),
      self.environment.clone(),
    ));

    Ok(plugin)
  }
}

struct WasmPluginFormatMessage {
  file_path: PathBuf,
  file_text: String,
  config: Arc<FormatConfig>,
  override_config: ConfigKeyMap,
}

type WasmResponseSender<T> = tokio::sync::oneshot::Sender<T>;

enum WasmPluginMessage {
  LicenseText(WasmResponseSender<Result<String>>),
  ResolvedConfig(Arc<FormatConfig>, WasmResponseSender<Result<String>>),
  ConfigDiagnostics(Arc<FormatConfig>, WasmResponseSender<Result<Vec<ConfigurationDiagnostic>>>),
  FormatRequest(Arc<WasmPluginFormatMessage>, WasmResponseSender<FormatResult>),
}

type WasmPluginSender = std::sync::mpsc::Sender<WasmPluginMessage>;

#[derive(Clone)]
struct InstanceState {
  host_format_callback: HostFormatCallback,
  token: Arc<dyn CancellationToken>,
}

struct WasmCreatedInstanceInfo {
  poisoner: Poisoner,
  handle: Option<JoinHandle<()>>,
}

struct WasmPluginSenderWithState {
  sender: Rc<WasmPluginSender>,
  instance_state_cell: Rc<RefCell<Option<InstanceState>>>,
  poisoner: Poisoner,
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

type LoadInstanceFn = dyn Fn(&mut wasmer::Store, &WasmModule, WasmHostFormatSender) -> Result<WasmInstance> + Send + Sync;

struct InitializedWasmPluginInner<TEnvironment: Environment> {
  name: String,
  created_instances: RefCell<Vec<WasmCreatedInstanceInfo>>,
  pending_instances: RefCell<Vec<WasmPluginSenderWithState>>,
  module: WasmModule,
  load_instance: Arc<LoadInstanceFn>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> Drop for InitializedWasmPluginInner<TEnvironment> {
  fn drop(&mut self) {
    let start = Instant::now();
    let len = {
      let instances = {
        let mut instances = self.pending_instances.borrow_mut();
        std::mem::take(&mut *instances)
      };

      instances.len()
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
  pub fn new(name: String, module: WasmModule, load_instance: Arc<LoadInstanceFn>, environment: TEnvironment) -> Self {
    Self(InitializedWasmPluginInner {
      name,
      pending_instances: Default::default(),
      created_instances: Default::default(),
      module,
      load_instance,
      environment,
    })
  }

  async fn with_instance<T>(
    &self,
    instance_state: Option<InstanceState>,
    action: impl Fn(Rc<WasmPluginSender>) -> LocalBoxFuture<'static, Result<T>>,
  ) -> Result<T> {
    let plugin = match self.get_or_create_instance(instance_state.clone()).await {
      Ok(instance) => instance,
      Err(err) => return Err(CriticalFormatError(err).into()),
    };
    let result = tokio::select! {
      result = action(plugin.sender.clone()) => result,
      _ = plugin.poisoner.wait_poisoned() => {
        bail!("poisoned")
      }
    };
    match result {
      Ok(result) => {
        self.release_instance(plugin);
        Ok(result)
      }
      Err(original_err) if original_err.downcast_ref::<CriticalFormatError>().is_some() => {
        let plugin = match self.get_or_create_instance(instance_state).await {
          Ok(plugin) => plugin,
          Err(err) => return Err(CriticalFormatError(err).into()),
        };

        // try again
        let result = tokio::select! {
          result = action(plugin.sender.clone()) => result,
          _ = plugin.poisoner.wait_poisoned() => {
            bail!("poisoned")
          }
        };
        match result {
          Ok(result) => {
            self.release_instance(plugin);
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
            self.release_instance(plugin);
            Err(err)
          }
        }
      }
      Err(err) => {
        self.release_instance(plugin);
        Err(err)
      }
    }
  }

  async fn get_or_create_instance(&self, instance_state: Option<InstanceState>) -> Result<WasmPluginSenderWithState> {
    let maybe_instance = self.0.pending_instances.borrow_mut().pop(); // needs to be on a separate line
    let plugin_sender = match maybe_instance {
      Some(instance) => instance,
      None => {
        let (instance, created_info) = self.create_instance().await?;
        self.0.created_instances.borrow_mut().push(created_info);
        instance
      }
    };
    *plugin_sender.instance_state_cell.borrow_mut() = instance_state;
    Ok(plugin_sender)
  }

  fn release_instance(&self, plugin_sender: WasmPluginSenderWithState) {
    *plugin_sender.instance_state_cell.borrow_mut() = None;
    self.0.pending_instances.borrow_mut().push(plugin_sender);
  }

  async fn create_instance(&self) -> Result<(WasmPluginSenderWithState, WasmCreatedInstanceInfo)> {
    let start_instant = Instant::now();
    log_verbose!(self.0.environment, "Creating instance of {}", self.0.name);
    let mut store = wasmer::Store::default();

    let (host_format_tx, mut host_format_rx) = tokio::sync::mpsc::unbounded_channel::<(HostFormatRequest, std::sync::mpsc::Sender<FormatResult>)>();
    let instance_state_cell: Rc<RefCell<Option<InstanceState>>> = Default::default();
    let poisoner = Poisoner::default();

    dprint_core::async_runtime::spawn({
      let instance_state_cell = instance_state_cell.clone();
      let poisoner = poisoner.clone();
      async move {
        while let Some((mut request, sender)) = host_format_rx.recv().await {
          let instance_state = instance_state_cell.borrow().clone();
          match instance_state {
            Some(instance_state) => {
              // forward the provided token on to the host format
              request.token = instance_state.token.clone();
              eprintln!("1");
              let message = (instance_state.host_format_callback)(request);
              eprintln!("2");
              tokio::select! {
                _ = poisoner.wait_poisoned() => {
                  eprintln!("EXITED");
                  // in this case, we're shutting down, so just quit
                  let _ = sender.send(Ok(None)); // poisoned
                  return; // quit
                }
                result = message => {
              eprintln!("333");
                  if sender.send(result).is_err() {
                    return; // disconnected
                  }
                }
              }
            }
            None => {
              if sender.send(Err(anyhow!("Host format callback was not set."))).is_err() {
                return; // disconnected
              }
            }
          }
        }
      }
    });

    let (tx, rx) = std::sync::mpsc::channel::<WasmPluginMessage>();
    let (initialize_tx, initialize_rx) = tokio::sync::oneshot::channel::<Result<(), anyhow::Error>>();

    // spawn the wasm instance on a dedicated thread to reduce issues
    let handle = dprint_core::async_runtime::spawn_blocking({
      let load_instance = self.0.load_instance.clone();
      let module = self.0.module.clone();
      move || {
        let initialize = || {
          let instance = (load_instance)(&mut store, &module, host_format_tx)?;
          let wasm_functions = WasmFunctions::new(store, instance)?;
          let instance = InitializedWasmPluginInstance::new(wasm_functions)?;
          Ok(instance)
        };
        let mut instance = match initialize() {
          Ok(instance) => {
            if initialize_tx.send(Ok(())).is_err() {
              return; // disconnected
            }
            instance
          }
          Err(err) => {
            let _ = initialize_tx.send(Err(err));
            return; // quit
          }
        };
        while let Ok(message) = rx.recv() {
          match message {
            WasmPluginMessage::LicenseText(response) => {
              let result = instance.license_text();
              if response.send(result).is_err() {
                break; // disconnected
              }
            }
            WasmPluginMessage::ConfigDiagnostics(config, response) => {
              let result = instance.config_diagnostics(&config);
              if response.send(result).is_err() {
                break; // disconnected
              }
            }
            WasmPluginMessage::ResolvedConfig(config, response) => {
              let result = instance.resolved_config(&config);
              if response.send(result).is_err() {
                break; // disconnected
              }
            }
            WasmPluginMessage::FormatRequest(request, response) => {
              let result = instance.format_text(&request.file_path, &request.file_text, &request.config, &request.override_config);
              if response.send(result).is_err() {
                break; // disconnected
              }
            }
          }
        }
      }
    });

    // wait for initialization
    initialize_rx.await??;

    log_verbose!(
      self.0.environment,
      "Created instance of {} in {}ms",
      self.0.name,
      start_instant.elapsed().as_millis() as u64
    );
    Ok((
      WasmPluginSenderWithState {
        sender: Rc::new(tx),
        instance_state_cell,
        poisoner: poisoner.clone(),
      },
      WasmCreatedInstanceInfo {
        poisoner,
        handle: Some(handle),
      },
    ))
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> InitializedPlugin for InitializedWasmPlugin<TEnvironment> {
  async fn license_text(&self) -> Result<String> {
    self
      .with_instance(None, move |plugin_sender| {
        async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          plugin_sender.send(WasmPluginMessage::LicenseText(tx))?;
          rx.await?
        }
        .boxed_local()
      })
      .await
  }

  async fn resolved_config(&self, config: Arc<FormatConfig>) -> Result<String> {
    self
      .with_instance(None, move |plugin_sender| {
        let config = config.clone();
        async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          plugin_sender.send(WasmPluginMessage::ResolvedConfig(config, tx))?;
          rx.await?
        }
        .boxed_local()
      })
      .await
  }

  async fn config_diagnostics(&self, config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>> {
    self
      .with_instance(None, move |plugin_sender| {
        let config = config.clone();
        async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          plugin_sender.send(WasmPluginMessage::ConfigDiagnostics(config, tx))?;
          rx.await?
        }
        .boxed_local()
      })
      .await
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
    let message = Arc::new(WasmPluginFormatMessage {
      file_path: request.file_path,
      file_text: request.file_text,
      config: request.config,
      override_config: request.override_config,
    });
    let instance_state = InstanceState {
      host_format_callback: request.on_host_format,
      token: request.token,
    };
    self
      .with_instance(Some(instance_state), move |plugin_sender| {
        let message = message.clone();
        async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          plugin_sender.send(WasmPluginMessage::FormatRequest(message, tx))?;
          rx.await?
        }
        .boxed_local()
      })
      .await
  }

  async fn shutdown(&self) {
    // drain the pending instances
    {
      self.0.pending_instances.borrow_mut().drain(..);
    }
    let created_instance_infos = self.0.created_instances.borrow_mut().drain(..).collect::<Vec<_>>();

    // poison all the created instances
    for info in &created_instance_infos {
      eprintln!("POISTONED");
      info.poisoner.poison();
    }

    // Now finally wait for everything to shut down nicely. This is necessary
    // because there might be a plugin that's stuck in host formatting and
    // now we want it to finish gracefully before the engine shuts down.
    for mut info in created_instance_infos {
      eprintln!("a1");
      let handle = info.handle.take();
      drop(info);
      eprintln!("a2");
      if let Some(handle) = handle {
        eprintln!("a3");
        handle.await.unwrap();
        eprintln!("a4");
      }
    }
  }
}
