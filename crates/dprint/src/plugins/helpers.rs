use std::sync::Arc;

use anyhow::Result;
use thiserror::Error;

use super::FormatConfig;
use super::InitializedPlugin;
use crate::environment::Environment;

#[derive(Debug, Error)]
#[error("[{}]: Error initializing from configuration file. Had {} diagnostic(s).", .plugin_name, .diagnostic_count)]
pub struct OutputPluginConfigDiagnosticsError {
  pub plugin_name: String,
  pub diagnostic_count: usize,
  /// The text of each diagnostic in the form it was logged.
  pub diagnostics: Vec<String>,
}

pub async fn output_plugin_config_diagnostics<TEnvironment: Environment>(
  plugin_name: &str,
  plugin: &dyn InitializedPlugin,
  format_config: Arc<FormatConfig>,
  environment: &TEnvironment,
) -> Result<Result<(), OutputPluginConfigDiagnosticsError>> {
  let mut diagnostics = Vec::new();

  for diagnostic in plugin.config_diagnostics(format_config).await? {
    let message = format!("[{}]: {}", plugin_name, diagnostic);
    log_warn!(environment, "{}", message);
    diagnostics.push(message);
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
