use std::collections::HashMap;
use dprint_core::configuration::{ConfigurationDiagnostic, ResolveConfigurationResult, GlobalConfiguration};
use std::path::PathBuf;
use dprint_core::plugins::*;
use super::configuration::{Configuration, resolve_config};
use super::formatter::Formatter;

/// TypeScript Dprint CLI Plugin.
pub struct TypeScriptPlugin {
    resolve_config_result: Option<ResolveConfigurationResult<Configuration>>,
    formatter: Option<Formatter>,
}

impl TypeScriptPlugin {
    pub fn new() -> TypeScriptPlugin {
        TypeScriptPlugin {
            resolve_config_result: None,
            formatter: None,
        }
    }

    fn get_resolved_config_result(&self) -> &ResolveConfigurationResult<Configuration> {
        self.resolve_config_result.as_ref().expect("Plugin must be initialized.")
    }

    fn get_formatter(&self) -> &Formatter {
        self.formatter.as_ref().expect("Plugin must be initialized.")
    }
}

impl Plugin for TypeScriptPlugin {
    fn name(&self) -> &'static str { env!("CARGO_PKG_NAME") }
    fn version(&self) -> &'static str { env!("CARGO_PKG_VERSION") }
    fn config_keys(&self) -> Vec<String> { vec![String::from("typescript"), String::from("javascript")] }

    fn initialize(&mut self, plugin_config: HashMap<String, String>, global_config: &GlobalConfiguration) {
        let config_result = resolve_config(plugin_config, &global_config);
        self.formatter = Some(Formatter::new(config_result.config.clone()));
        self.resolve_config_result = Some(config_result);
    }

    fn should_format_file(&self, file_path: &PathBuf, _: &str) -> bool {
        if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
            match String::from(ext).to_lowercase().as_ref() {
                "js" | "jsx" | "ts" | "tsx" => true,
                _ => false,
            }
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

    fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, String> {
        self.get_formatter().format_text(file_path, file_text)
    }
}
