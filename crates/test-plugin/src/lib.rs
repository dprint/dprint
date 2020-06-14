use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    ending: String,
}

pub fn resolve_config(config: HashMap<String, String>, _: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut config = config;
    let ending = config.remove("ending").unwrap_or(String::from("formatted"));
    let mut diagnostics = Vec::new();

    diagnostics.extend(get_unknown_property_diagnostics(config));

    ResolveConfigurationResult {
        config: Configuration { ending },
        diagnostics,
    }
}

fn get_plugin_config_key() -> String {
    String::from("test-plugin")
}

fn get_plugin_file_extensions() -> Vec<String> {
    vec![String::from("txt")]
}

fn get_plugin_help_url() -> String {
    String::from("https://dprint.dev/plugins/test")
}

fn get_plugin_config_schema_url() -> String {
    String::from("https://plugins.dprint.dev/schemas/test.json")
}

fn get_plugin_license_text() -> String {
    std::str::from_utf8(include_bytes!("../LICENSE")).unwrap().into()
}

fn format_text(_: &PathBuf, file_text: &str, config: &Configuration) -> Result<String, String> {
    if file_text.ends_with(&config.ending) {
        Ok(String::from(file_text))
    } else {
        Ok(format!("{}_{}", file_text, config.ending))
    }
}

generate_plugin_code!();
