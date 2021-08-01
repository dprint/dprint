use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics, ConfigKeyMap, get_value};
use dprint_core::{err_obj, err};
use dprint_core::types::ErrBox;
use dprint_core::plugins::{PluginHandler, PluginInfo};
use dprint_core::plugins::process::{get_parent_process_id_from_cli_args, handle_process_stdio_messages, start_parent_process_checker_thread};

fn main() -> Result<(), ErrBox> {
    if let Some(parent_process_id) = get_parent_process_id_from_cli_args() {
        start_parent_process_checker_thread(parent_process_id);
    }

    handle_process_stdio_messages(TestProcessPluginHandler::new())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

struct TestProcessPluginHandler {
}

impl TestProcessPluginHandler {
    fn new() -> Self {
        TestProcessPluginHandler {}
    }
}

impl PluginHandler<Configuration> for TestProcessPluginHandler {
    fn get_plugin_info(&mut self) -> PluginInfo {
        PluginInfo {
            name: String::from(env!("CARGO_PKG_NAME")),
            version: String::from(env!("CARGO_PKG_VERSION")),
            config_key: "testProcessPlugin".to_string(),
            file_extensions: vec!["txt_ps".to_string()],
            file_names: vec!["test-process-plugin-exact-file".to_string()],
            help_url: "https://dprint.dev/plugins/test-process".to_string(),
            config_schema_url: "".to_string()
        }
    }

    fn get_license_text(&mut self) -> String {
        "License text.".to_string()
    }

    fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
        let mut config = config;
        let mut diagnostics = Vec::new();
        let ending = get_value(&mut config, "ending", String::from("formatted_process"), &mut diagnostics);
        let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

        diagnostics.extend(get_unknown_property_diagnostics(config));

        ResolveConfigurationResult {
            config: Configuration { ending, line_width },
            diagnostics,
        }
    }

    fn format_text(
        &mut self,
        _: &Path,
        file_text: &str,
        config: &Configuration,
        mut format_with_host: impl FnMut(&Path, String, &ConfigKeyMap) -> Result<String, ErrBox>,
    ) -> Result<String, ErrBox> {
        if file_text.starts_with("plugin: ") {
            format_with_host(&PathBuf::from("./test.txt"), file_text.replace("plugin: ", ""), &HashMap::new())
        } else if file_text.starts_with("plugin-config: ") {
            let mut config_map = HashMap::new();
            config_map.insert("ending".to_string(), "custom_config".into());
            format_with_host(&PathBuf::from("./test.txt"), file_text.replace("plugin-config: ", ""), &config_map)
        } else if file_text == "should_error" {
            err!("Did error.")
        } else if file_text.ends_with(&config.ending) {
            Ok(String::from(file_text))
        } else {
            Ok(format!("{}_{}", file_text, config.ending))
        }
    }
}
