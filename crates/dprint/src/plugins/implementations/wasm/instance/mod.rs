use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION;
use wasmtime::Engine;
use wasmtime::Memory;

use crate::environment::Environment;
use crate::plugins::FormatConfig;

use super::WasmInstance;

mod v3;
mod v4;

pub type WasmHostFormatSender = tokio::sync::mpsc::UnboundedSender<(HostFormatRequest, std::sync::mpsc::Sender<FormatResult>)>;

/// The data carried by the wasmtime `Store`. Host state lives inside the store
/// and host functions reach it through `Caller::data`/`data_mut`. A given store
/// only ever runs one schema version, so the host functions match on their variant.
pub enum WasmHostState {
  /// Used for the identity import object (compilation / plugin-info probing)
  /// where the host functions are no-ops, so no state is needed.
  Empty,
  V3(v3::ImportObjectEnvironmentV3),
  V4(v4::ImportObjectEnvironmentV4),
}

impl WasmHostState {
  pub fn set_memory(&mut self, memory: Memory) {
    match self {
      WasmHostState::Empty => {}
      WasmHostState::V3(state) => state.memory = Some(memory),
      WasmHostState::V4(state) => state.memory = Some(memory),
    }
  }

  pub fn set_token(&mut self, token: Arc<dyn CancellationToken>) {
    match self {
      WasmHostState::Empty => {}
      WasmHostState::V3(state) => state.token = token,
      WasmHostState::V4(state) => state.token = token,
    }
  }
}

pub type Store = wasmtime::Store<WasmHostState>;
pub type Linker = wasmtime::Linker<WasmHostState>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PluginSchemaVersion {
  V3,
  V4,
}

pub trait InitializedWasmPluginInstance {
  fn plugin_info(&mut self) -> Result<PluginInfo>;
  fn license_text(&mut self) -> Result<String>;
  fn resolved_config(&mut self, config: &FormatConfig) -> Result<String>;
  fn config_diagnostics(&mut self, config: &FormatConfig) -> Result<Vec<ConfigurationDiagnostic>>;
  fn file_matching_info(&mut self, config: &FormatConfig) -> Result<FileMatchingInfo>;
  fn check_config_updates(&mut self, message: &CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>>;
  fn format_text(
    &mut self,
    file_path: &Path,
    file_bytes: &[u8],
    range: FormatRange,
    config: &FormatConfig,
    override_config: &ConfigKeyMap,
    token: Arc<dyn CancellationToken>,
  ) -> FormatResult;
}

pub fn create_wasm_plugin_instance(store: Store, instance: WasmInstance) -> Result<Box<dyn InitializedWasmPluginInstance>> {
  match instance.version() {
    PluginSchemaVersion::V3 => Ok(Box::new(v3::InitializedWasmPluginInstanceV3::new(store, instance)?)),
    PluginSchemaVersion::V4 => Ok(Box::new(v4::InitializedWasmPluginInstanceV4::new(store, instance)?)),
  }
}

/// Builds a linker whose host functions are no-ops. Used when the plugin doesn't
/// need to format via a plugin pool (compilation and plugin-info probing).
pub fn create_identity_import_object(version: PluginSchemaVersion, engine: &Engine) -> Result<Linker> {
  let mut linker = Linker::new(engine);
  match version {
    PluginSchemaVersion::V3 => v3::add_identity_imports(&mut linker)?,
    PluginSchemaVersion::V4 => v4::add_identity_imports(&mut linker)?,
  }
  Ok(linker)
}

/// Builds a linker plus the initial store state for an instance that formats
/// text using plugins from the plugin pool.
pub fn create_pools_import_object<TEnvironment: Environment>(
  environment: TEnvironment,
  plugin_name: &str,
  version: PluginSchemaVersion,
  engine: &Engine,
  host_format_sender: WasmHostFormatSender,
) -> Result<(Linker, WasmHostState)> {
  match version {
    PluginSchemaVersion::V3 => v3::create_pools_import_object(engine, host_format_sender),
    PluginSchemaVersion::V4 => v4::create_pools_import_object(environment, plugin_name.to_string(), engine, host_format_sender),
  }
}

pub fn get_current_plugin_schema_version(module: &wasmtime::Module) -> Result<PluginSchemaVersion> {
  fn from_exports(module: &wasmtime::Module) -> Result<u32> {
    for export in module.exports() {
      let name = export.name();
      if matches!(name, "get_plugin_schema_version") {
        // not exactly correct, but practically ok because this has been returning v3 for many years
        return Ok(3);
      } else if let Some(version) = name.strip_prefix("dprint_plugin_version_") {
        // this is what dprint will use in the future
        if let Ok(version) = version.parse() {
          return Ok(version);
        }
      }
    }
    bail!("Error determining plugin schema version. Are you sure this is a dprint plugin? If so, maybe try upgrading dprint.");
  }

  let plugin_schema_version = from_exports(module)?;
  match plugin_schema_version {
    3 => Ok(PluginSchemaVersion::V3),
    4 => Ok(PluginSchemaVersion::V4),
    version if version > 4 => {
      bail!(
        "Invalid schema version: {} -- Expected: {}. Upgrade your dprint CLI ({}).",
        plugin_schema_version,
        PLUGIN_SYSTEM_SCHEMA_VERSION,
        get_current_exe_display(),
      );
    }
    plugin_schema_version => {
      bail!(
        "Invalid schema version: {} -- Expected: {}. This plugin is too old for your version of dprint ({}). Please update the plugin manually.",
        plugin_schema_version,
        PLUGIN_SYSTEM_SCHEMA_VERSION,
        get_current_exe_display(),
      );
    }
  }
}

fn get_current_exe_display() -> String {
  std::env::current_exe()
    .ok()
    .map(|p| p.display().to_string())
    .unwrap_or_else(|| "<unknown path>".to_string())
}
