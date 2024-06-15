use anyhow::anyhow;
use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::PluginInfo;

use super::create_pools_import_object;
use super::load_instance;
use super::load_instance::WasmInstance;
use super::load_instance::WasmModule;
use super::WasmHostFormatSender;
use super::WasmModuleCreator;
use crate::environment::Environment;
use crate::plugins::implementations::wasm::create_wasm_plugin_instance;
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
        move |store, module, host_format_sender| {
          let (import_object, env) = create_pools_import_object(module.version(), store, host_format_sender);
          load_instance(store, module, env, &import_object)
        }
      }),
      self.environment.clone(),
    ));

    Ok(plugin)
  }
}

struct WasmPluginFormatMessage {
  file_path: PathBuf,
  file_bytes: Vec<u8>,
  config: Arc<FormatConfig>,
  override_config: ConfigKeyMap,
  token: Arc<dyn CancellationToken>,
}

type WasmResponseSender<T> = tokio::sync::oneshot::Sender<T>;

enum WasmPluginMessage {
  LicenseText(WasmResponseSender<Result<String>>),
  ResolvedConfig(Arc<FormatConfig>, WasmResponseSender<Result<String>>),
  FileMatchingInfo(Arc<FormatConfig>, WasmResponseSender<Result<FileMatchingInfo>>),
  ConfigDiagnostics(Arc<FormatConfig>, WasmResponseSender<Result<Vec<ConfigurationDiagnostic>>>),
  FormatRequest(Arc<WasmPluginFormatMessage>, WasmResponseSender<FormatResult>),
}

type WasmPluginSender = std::sync::mpsc::Sender<WasmPluginMessage>;

#[derive(Clone)]
struct InstanceState {
  host_format_callback: HostFormatCallback,
}

struct WasmPluginSenderWithState {
  sender: Rc<WasmPluginSender>,
  instance_state_cell: Rc<RefCell<Option<InstanceState>>>,
}

type LoadInstanceFn = dyn Fn(&mut wasmer::Store, &WasmModule, WasmHostFormatSender) -> Result<WasmInstance> + Send + Sync;

pub struct InitializedWasmPlugin<TEnvironment: Environment> {
  name: String,
  pending_instances: RefCell<Vec<WasmPluginSenderWithState>>,
  module: WasmModule,
  load_instance: Arc<LoadInstanceFn>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> Drop for InitializedWasmPlugin<TEnvironment> {
  fn drop(&mut self) {
    let start = Instant::now();
    let len = {
      let instances = {
        let mut instances = self.pending_instances.borrow_mut();
        std::mem::take(&mut *instances)
      };

      instances.len()
    };
    log_debug!(
      self.environment,
      "Dropped {} ({} instances) in {}ms",
      self.name,
      len,
      start.elapsed().as_millis()
    );
  }
}

impl<TEnvironment: Environment> InitializedWasmPlugin<TEnvironment> {
  pub fn new(name: String, module: WasmModule, load_instance: Arc<LoadInstanceFn>, environment: TEnvironment) -> Self {
    Self {
      name,
      pending_instances: Default::default(),
      module,
      load_instance,
      environment,
    }
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
    let result = action(plugin.sender.clone()).await;
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
        let result = action(plugin.sender.clone()).await;
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
              self.name,
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
    let maybe_instance = self.pending_instances.borrow_mut().pop(); // needs to be on a separate line
    let plugin_sender = match maybe_instance {
      Some(instance) => instance,
      None => self.create_instance().await?,
    };
    *plugin_sender.instance_state_cell.borrow_mut() = instance_state;
    Ok(plugin_sender)
  }

  fn release_instance(&self, plugin_sender: WasmPluginSenderWithState) {
    *plugin_sender.instance_state_cell.borrow_mut() = None;
    self.pending_instances.borrow_mut().push(plugin_sender);
  }

  async fn create_instance(&self) -> Result<WasmPluginSenderWithState> {
    let start_instant = Instant::now();
    log_debug!(self.environment, "Creating instance of {}", self.name);
    let mut store = wasmer::Store::default();

    let (host_format_tx, mut host_format_rx) = tokio::sync::mpsc::unbounded_channel::<(HostFormatRequest, std::sync::mpsc::Sender<FormatResult>)>();
    let instance_state_cell: Rc<RefCell<Option<InstanceState>>> = Default::default();

    dprint_core::async_runtime::spawn({
      let instance_state_cell = instance_state_cell.clone();
      async move {
        while let Some((request, sender)) = host_format_rx.recv().await {
          let instance_state = instance_state_cell.borrow().clone();
          match instance_state {
            Some(instance_state) => {
              let message = (instance_state.host_format_callback)(request).await;
              if sender.send(message).is_err() {
                return; // disconnected
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
    dprint_core::async_runtime::spawn_blocking({
      let load_instance = self.load_instance.clone();
      let module = self.module.clone();
      move || {
        let initialize = || {
          let instance = (load_instance)(&mut store, &module, host_format_tx)?;
          let instance = create_wasm_plugin_instance(store, instance)?;
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
            WasmPluginMessage::FileMatchingInfo(config, response) => {
              let result = instance.file_matching_info(&config);
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
              let result = instance.format_text(
                &request.file_path,
                &request.file_bytes,
                &request.config,
                &request.override_config,
                request.token.clone(),
              );
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

    log_debug!(
      self.environment,
      "Created instance of {} in {}ms",
      self.name,
      start_instant.elapsed().as_millis() as u64
    );
    Ok(WasmPluginSenderWithState {
      sender: Rc::new(tx),
      instance_state_cell,
    })
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

  async fn file_matching_info(&self, config: Arc<FormatConfig>) -> Result<FileMatchingInfo> {
    self
      .with_instance(None, move |plugin_sender| {
        let config = config.clone();
        async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          plugin_sender.send(WasmPluginMessage::FileMatchingInfo(config, tx))?;
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

  async fn check_config_updates(&self, _plugin_config: ConfigKeyMap) -> Result<Vec<ConfigChange>> {
    Ok(Vec::new()) // not supported atm
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
      file_bytes: request.file_text,
      config: request.config,
      override_config: request.override_config,
      token: request.token,
    });
    let instance_state = InstanceState {
      host_format_callback: request.on_host_format,
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
    // do nothing
  }
}
