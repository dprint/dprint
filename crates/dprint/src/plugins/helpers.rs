use std::sync::Arc;

use anyhow::Result;
use dprint_core::configuration::ConfigurationDiagnostic;
use thiserror::Error;

use super::FormatConfig;
use super::InitializedPlugin;
use crate::environment::Environment;

#[derive(Debug, Error)]
#[error("[{}]: Error initializing from configuration file. Had {} diagnostic(s).", .plugin_name, .diagnostic_count)]
pub struct OutputPluginConfigDiagnosticsError {
  pub plugin_name: String,
  pub diagnostic_count: usize,
  pub diagnostics: Vec<PluginConfigDiagnostic>,
}

/// A problem with the configuration of a plugin, which is displayed in the
/// form it's logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginConfigDiagnostic {
  pub plugin_name: String,
  pub diagnostic: ConfigurationDiagnostic,
}

impl std::fmt::Display for PluginConfigDiagnostic {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "[{}]: {}", self.plugin_name, self.diagnostic)
  }
}

pub async fn output_plugin_config_diagnostics<TEnvironment: Environment>(
  plugin_name: &str,
  plugin: &dyn InitializedPlugin,
  format_config: Arc<FormatConfig>,
  environment: &TEnvironment,
) -> Result<Result<(), OutputPluginConfigDiagnosticsError>> {
  let mut diagnostics = Vec::new();

  for diagnostic in plugin.config_diagnostics(format_config).await? {
    let diagnostic = PluginConfigDiagnostic {
      plugin_name: plugin_name.to_string(),
      diagnostic,
    };
    log_warn!(environment, "{}", diagnostic);
    diagnostics.push(diagnostic);
  }

  if !diagnostics.is_empty() {
    Ok(Err(OutputPluginConfigDiagnosticsError {
      plugin_name: plugin_name.to_string(),
      diagnostic_count: diagnostics.len(),
      diagnostics,
    }))
  } else {
    Ok(Ok(()))
  }
}
