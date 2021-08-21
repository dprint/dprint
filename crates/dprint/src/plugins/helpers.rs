use dprint_cli_core::types::ErrBox;

use super::InitializedPlugin;
use crate::environment::Environment;
use crate::utils::ErrorCountLogger;

pub fn output_plugin_config_diagnostics<TEnvironment: Environment>(
  plugin_name: &str,
  plugin: &Box<dyn InitializedPlugin>,
  error_logger: &ErrorCountLogger<TEnvironment>,
) -> Result<(), ErrBox> {
  let mut diagnostic_count = 0;

  for diagnostic in plugin.get_config_diagnostics()? {
    error_logger.log_error(&format!("[{}]: {}", plugin_name, diagnostic.message));
    diagnostic_count += 1;
  }

  if diagnostic_count > 0 {
    err!(
      "[{}]: Error initializing from configuration file. Had {} diagnostic(s).",
      plugin_name,
      diagnostic_count
    )
  } else {
    Ok(())
  }
}
