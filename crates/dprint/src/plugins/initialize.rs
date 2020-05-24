use std::collections::HashMap;
use dprint_core::configuration::GlobalConfiguration;

use crate::environment::Environment;
use crate::types::ErrBox;
use super::{Plugin, InitializedPlugin};

pub fn initialize_plugin(
    plugin: Box<dyn Plugin>,
    plugin_config: HashMap<String, String>,
    global_config: &GlobalConfiguration,
    environment: &impl Environment,
) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
    let mut plugin = plugin;
    let initialized_plugin = plugin.initialize(plugin_config, &global_config)?;
    let mut diagnostic_count = 0;

    for diagnostic in initialized_plugin.get_config_diagnostics() {
        environment.log_error(&format!("[{}]: {}", plugin.name(), diagnostic.message));
        diagnostic_count += 1;
    }

    if diagnostic_count > 0 {
        err!("Error initializing from configuration file. Had {} diagnostic(s).", diagnostic_count)
    } else {
        Ok(initialized_plugin)
    }
}
