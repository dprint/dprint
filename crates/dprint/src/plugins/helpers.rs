use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;

use super::InitializedPlugin;
use crate::environment::Environment;
use crate::utils::ErrorCountLogger;

pub async fn output_plugin_config_diagnostics<TEnvironment: Environment>(
  plugin_name: &str,
  plugin: Arc<dyn InitializedPlugin>,
  error_logger: ErrorCountLogger<TEnvironment>,
) -> Result<()> {
  let mut diagnostic_count = 0;

  for diagnostic in plugin.config_diagnostics().await? {
    error_logger.log_error(&format!("[{}]: {}", plugin_name, diagnostic));
    diagnostic_count += 1;
  }

  if diagnostic_count > 0 {
    bail!(
      "[{}]: Error initializing from configuration file. Had {} diagnostic(s).",
      plugin_name,
      diagnostic_count
    )
  } else {
    Ok(())
  }
}
