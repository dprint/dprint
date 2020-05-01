use std::collections::HashMap;
use dprint_core::configuration::{ConfigurationDiagnostic, ResolveConfigurationResult, GlobalConfiguration};
use std::path::PathBuf;
use dprint_core::plugins::*;
use super::configuration::{Configuration, resolve_config};
use super::format_text::format_text;

/// JSONC Dprint CLI Plugin.
pub struct JsoncPlugin {
    resolve_config_result: Option<ResolveConfigurationResult<Configuration>>,
}

impl JsoncPlugin {
    pub fn new() -> JsoncPlugin {
        JsoncPlugin {
            resolve_config_result: None,
        }
    }

    fn get_resolved_config_result(&self) -> &ResolveConfigurationResult<Configuration> {
        self.resolve_config_result.as_ref().expect("Plugin must be initialized.")
    }
}

impl Plugin for JsoncPlugin {
    fn name(&self) -> &'static str { env!("CARGO_PKG_NAME") }
    fn version(&self) -> &'static str { env!("CARGO_PKG_VERSION") }
    fn config_keys(&self) -> Vec<String> { vec![String::from("json"), String::from("jsonc")] }

    fn initialize(&mut self, plugin_config: HashMap<String, String>, global_config: &GlobalConfiguration) {
        self.resolve_config_result = Some(resolve_config(plugin_config, &global_config));
    }

    fn should_format_file(&self, file_path: &PathBuf, _: &str) -> bool {
        if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
            String::from(ext).to_lowercase() == "json"
        } else {
            false
        }
    }

    fn get_resolved_config(&self) -> String {
        let config = &self.get_resolved_config_result().config;
        serde_json::to_string_pretty(config).unwrap()
    }

    fn get_configuration_diagnostics(&self) -> &Vec<ConfigurationDiagnostic> {
        &self.get_resolved_config_result().diagnostics
    }

    fn format_text(&self, _: &PathBuf, file_text: &str) -> Result<String, String> {
        let config = &self.get_resolved_config_result().config;
        format_text(file_text, config)
    }
}
