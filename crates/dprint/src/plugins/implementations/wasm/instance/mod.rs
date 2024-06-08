use std::path::Path;

use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use v1::InitializedWasmPluginInstanceV1;
use wasmer::Store;

use crate::plugins::FormatConfig;

use super::WasmInstance;

mod v1;

pub trait InitializedWasmPluginInstance {
  fn plugin_info(&mut self) -> Result<PluginInfo>;
  fn license_text(&mut self) -> Result<String>;
  fn resolved_config(&mut self, config: &FormatConfig) -> Result<String>;
  fn config_diagnostics(&mut self, config: &FormatConfig) -> Result<Vec<ConfigurationDiagnostic>>;
  fn file_matching_info(&mut self, _config: &FormatConfig) -> Result<FileMatchingInfo>;
  fn format_text(&mut self, file_path: &Path, file_bytes: &[u8], config: &FormatConfig, override_config: &ConfigKeyMap) -> FormatResult;
}

pub fn create_wasm_plugin_instance(store: Store, instance: WasmInstance) -> Result<Box<dyn InitializedWasmPluginInstance>> {
  match instance.version() {
    PluginSchemaVersion::V3 => Ok(Box::new(InitializedWasmPluginInstanceV1::new(store, instance)?)),
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PluginSchemaVersion {
  V3,
}

pub fn get_current_plugin_schema_version(module: &wasmer::Module) -> Result<PluginSchemaVersion> {
  fn from_exports(module: &wasmer::Module) -> Result<u32> {
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
    PLUGIN_SYSTEM_SCHEMA_VERSION => Ok(PluginSchemaVersion::V3),
    version if version > PLUGIN_SYSTEM_SCHEMA_VERSION => {
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
