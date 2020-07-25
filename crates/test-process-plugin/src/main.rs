use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics};
use dprint_core::err;
use dprint_core::types::ErrBox;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::{handle_process_stdin_stdout_messages, ProcessPluginHandler};

fn main() -> Result<(), ErrBox> {
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

    fn resolve_config(&self, config: HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
        let mut config = config;
        let ending = config.remove("ending").unwrap_or(String::from("formatted_process"));
        let line_width = config.remove("line_width").map(|x| x.parse::<u32>().unwrap()).unwrap_or(global_config.line_width.unwrap_or(120));
        let mut diagnostics = Vec::new();

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
        format_with_host: Box<dyn FnMut(&PathBuf, String) -> Result<String, ErrBox> + 'a>,
    ) -> Result<String, ErrBox> {
        if file_text.starts_with("plugin: ") {
            let mut format_with_host = format_with_host;
            format_with_host(&PathBuf::from("./test.txt"), file_text.replace("plugin: ", ""))
        } else if file_text == "should_error" {
            err!("Did error.")
        } else if file_text.ends_with(&config.ending) {
            Ok(String::from(file_text))
        } else {
            Ok(format!("{}_{}", file_text, config.ending))
        }
    }
}
