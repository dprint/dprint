use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;

use super::InitializedProcessPluginCommunicator;

/// Use this to get an executable file name that also works in the tests.
///
/// In the test environment the cached "executable" is an in-memory copy of the
/// real test process plugin binary, which can't actually be launched. Copy it
/// out to a real temp file (named per version, since the test binary reports
/// its version from its own path — e.g. `temp-plugin-0.3.0`) and run that. The
/// cache layout no longer carries the version in the path, so it's passed in.
pub fn get_test_safe_executable_path(version: &str, executable_file_path: PathBuf, environment: &impl Environment) -> PathBuf {
  if environment.is_real() {
    return executable_file_path;
  }

  static CREATED_TEMP_FILES: once_cell::sync::Lazy<Mutex<std::collections::HashSet<PathBuf>>> = once_cell::sync::Lazy::new(Default::default);

  let tmp_dir = PathBuf::from("temp");
  let file_name = if cfg!(target_os = "windows") {
    format!("temp-plugin-{version}.exe")
  } else {
    format!("temp-plugin-{version}")
  };
  let temp_file = tmp_dir.join(file_name);

  // create the per-version temp executable once, from the (identical across
  // every test process plugin) binary bytes
  let mut created = CREATED_TEMP_FILES.lock();
  if created.insert(temp_file.clone()) {
    #[allow(clippy::disallowed_methods)]
    let _ = std::fs::create_dir(&tmp_dir);
    let bytes = environment.read_file_bytes(&executable_file_path).unwrap();
    #[allow(clippy::disallowed_methods)]
    let _ = std::fs::write(&temp_file, &bytes);
    if cfg!(unix) {
      std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("chmod +x {}", temp_file.to_string_lossy()))
        .status()
        .unwrap();
    }
  }
  temp_file
}

pub struct ProcessPlugin<TEnvironment: Environment> {
  environment: TEnvironment,
  executable_file_path: PathBuf,
  plugin_info: PluginInfo,
}

impl<TEnvironment: Environment> ProcessPlugin<TEnvironment> {
  pub fn new(environment: TEnvironment, executable_file_path: PathBuf, plugin_info: PluginInfo) -> Self {
    ProcessPlugin {
      environment,
      executable_file_path,
      plugin_info,
    }
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> Plugin for ProcessPlugin<TEnvironment> {
  fn info(&self) -> &PluginInfo {
    &self.plugin_info
  }

  fn is_process_plugin(&self) -> bool {
    true
  }

  async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>> {
    let start_instant = Instant::now();
    let plugin_name = &self.info().name;
    log_debug!(self.environment, "Creating instance of {}", plugin_name);
    let communicator = InitializedProcessPluginCommunicator::new(plugin_name.to_string(), self.executable_file_path.clone(), self.environment.clone()).await?;
    let process_plugin = InitializedProcessPlugin::new(communicator)?;

    let result: Rc<dyn InitializedPlugin> = Rc::new(process_plugin);
    log_debug!(
      self.environment,
      "Created instance of {} in {}ms",
      plugin_name,
      start_instant.elapsed().as_millis() as u64
    );
    Ok(result)
  }
}

pub struct InitializedProcessPlugin<TEnvironment: Environment> {
  communicator: Rc<InitializedProcessPluginCommunicator<TEnvironment>>,
}

impl<TEnvironment: Environment> InitializedProcessPlugin<TEnvironment> {
  pub fn new(communicator: InitializedProcessPluginCommunicator<TEnvironment>) -> Result<Self> {
    Ok(Self {
      communicator: Rc::new(communicator),
    })
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> InitializedPlugin for InitializedProcessPlugin<TEnvironment> {
  async fn license_text(&self) -> Result<String> {
    self.communicator.get_license_text().await
  }

  async fn resolved_config(&self, config: Arc<FormatConfig>) -> Result<String> {
    self.communicator.get_resolved_config(&config).await
  }

  async fn file_matching_info(&self, config: Arc<FormatConfig>) -> Result<FileMatchingInfo> {
    self.communicator.get_file_matching_info(&config).await
  }

  async fn config_diagnostics(&self, config: Arc<FormatConfig>) -> Result<Vec<ConfigurationDiagnostic>> {
    self.communicator.get_config_diagnostics(&config).await
  }

  async fn check_config_updates(&self, message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    self.communicator.check_config_updates(&message).await
  }

  async fn format_text(&self, request: InitializedPluginFormatRequest) -> FormatResult {
    self.communicator.format_text(request).await
  }

  async fn shutdown(&self) -> () {
    self.communicator.shutdown().await
  }
}
