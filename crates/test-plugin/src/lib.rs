use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics, ConfigKeyMap, get_value};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

fn resolve_config(config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut config = config;
    let mut diagnostics = Vec::new();
    let ending = get_value(&mut config, "ending", String::from("formatted"), &mut diagnostics);
    let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

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
    if file_text.starts_with("plugin: ") {
        format_with_host(&PathBuf::from("./test.txt_ps"), file_text.replace("plugin: ", ""), &HashMap::new())
    } else if file_text.starts_with("plugin-config: ") {
        let mut config_map = HashMap::new();
        config_map.insert("ending".to_string(), "custom_config".into());
        format_with_host(&PathBuf::from("./test.txt_ps"), file_text.replace("plugin-config: ", ""), &config_map)
    } else if file_text == "should_error" {
        Err(String::from("Did error."))
    } else if file_text.ends_with(&config.ending) {
        Ok(String::from(file_text))
    } else {
        Ok(format!("{}_{}", file_text, config.ending))
    }
}

generate_plugin_code!();
