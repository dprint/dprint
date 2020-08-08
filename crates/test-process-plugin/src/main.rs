use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics, ConfigKeyMap, get_value};
use dprint_core::err;
use dprint_core::types::ErrBox;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::{handle_process_stdin_stdout_messages, ProcessPluginHandler, start_parent_process_checker_thread};

fn main() -> Result<(), ErrBox> {
    if let Some(parent_process_id) = get_parent_process_id_from_args() {
        start_parent_process_checker_thread(String::from(env!("CARGO_PKG_NAME")), parent_process_id);
    }

    handle_process_stdin_stdout_messages(TestProcessPluginHandler::new())
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

impl ProcessPluginHandler<Configuration> for TestProcessPluginHandler {
    fn get_plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: String::from(env!("CARGO_PKG_NAME")),
            version: String::from(env!("CARGO_PKG_VERSION")),
            config_key: "testProcessPlugin".to_string(),
            file_extensions: vec!["txt_ps".to_string()],
            help_url: "https://dprint.dev/plugins/test-process".to_string(),
            config_schema_url: "".to_string()
        }
    }

    fn get_license_text(&self) -> &str {
        "License text."
    }

    fn resolve_config(&self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
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

    fn format_text<'a>(
        &'a self,
        _: &PathBuf,
        file_text: &str,
        config: &Configuration,
        format_with_host: Box<dyn FnMut(&PathBuf, String, &ConfigKeyMap) -> Result<String, ErrBox> + 'a>,
    ) -> Result<String, ErrBox> {
        if file_text.starts_with("plugin: ") {
            let mut format_with_host = format_with_host;
            format_with_host(&PathBuf::from("./test.txt"), file_text.replace("plugin: ", ""), &HashMap::new())
        } else if file_text.starts_with("plugin-config: ") {
            let mut config_map = HashMap::new();
            config_map.insert("ending".to_string(), "custom_config".into());
            let mut format_with_host = format_with_host;
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

fn get_parent_process_id_from_args() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--parent-pid" {
            if let Some(parent_pid) = args.get(i + 1) {
                return parent_pid.parse::<u32>().map(|x| Some(x)).unwrap_or(None);
            }
        }
    }

    return None;
}
