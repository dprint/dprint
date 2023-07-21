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
}

pub async fn output_plugin_config_diagnostics<TEnvironment: Environment>(
  plugin_name: &str,
  plugin: &dyn InitializedPlugin,
  format_config: Arc<FormatConfig>,
  environment: &TEnvironment,
) -> Result<Result<(), OutputPluginConfigDiagnosticsError>> {
  let mut diagnostic_count = 0;

  for diagnostic in plugin.config_diagnostics(format_config).await? {
    environment.log_stderr(&format!("[{}]: {}", plugin_name, diagnostic));
    diagnostic_count += 1;
  }

  if diagnostic_count > 0 {
    Ok(Err(OutputPluginConfigDiagnosticsError {
      plugin_name: plugin_name.to_string(),
      diagnostic_count,
    }))
  } else {
    Ok(Ok(()))
  }
}
