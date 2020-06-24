use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

fn resolve_config(config: HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut config = config;
    let ending = config.remove("ending").unwrap_or(String::from("formatted"));
    let line_width = config.remove("line_width").map(|x| x.parse::<u32>().unwrap()).unwrap_or(global_config.line_width.unwrap_or(120));
    let mut diagnostics = Vec::new();

    diagnostics.extend(get_unknown_property_diagnostics(config));

    ResolveConfigurationResult {
        config: Configuration { ending, line_width },
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
    if file_text == "should_error" {
        Err(String::from("Did error."))
    } else if file_text.ends_with(&config.ending) {
        Ok(String::from(file_text))
    } else {
        Ok(format!("{}_{}", file_text, config.ending))
    }
}

generate_plugin_code!();
