#[macro_use(err_obj)]
#[macro_use(err)]
extern crate dprint_core;

use std::path::{PathBuf, Path};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::types::ErrBox;
use dprint_core::plugins::{PluginHandler, PluginInfo};
use dprint_core::configuration::{GlobalConfiguration, ResolveConfigurationResult, get_unknown_property_diagnostics, ConfigKeyMap, get_value};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    ending: String,
    line_width: u32,
}

struct TestWasmPlugin {
    has_panicked: bool,
}

impl TestWasmPlugin {
    pub const fn new() -> Self {
        TestWasmPlugin {
            has_panicked: false,
        }
    }
}

impl PluginHandler<Configuration> for TestWasmPlugin {
    fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
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

    fn get_plugin_info(&mut self) -> PluginInfo {
        PluginInfo {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            config_key: "test-plugin".to_string(),
            file_extensions: vec!["txt".to_string()],
            file_names: vec![],
            help_url: "https://dprint.dev/plugins/test".to_string(),
            config_schema_url: "https://plugins.dprint.dev/schemas/test.json".to_string()
        }
    }

    fn get_license_text(&mut self) -> String {
        std::str::from_utf8(include_bytes!("../LICENSE")).unwrap().into()
    }

    fn format_text(
        &mut self,
        _: &Path,
        file_text: &str,
        config: &Configuration,
        mut format_with_host: impl FnMut(&Path, String, &ConfigKeyMap) -> Result<String, ErrBox>,
    ) -> Result<String, ErrBox> {
        if self.has_panicked {
            panic!("Previously panicked. Plugin should not have been used by the CLI again.")
        } else if file_text.starts_with("plugin: ") {
            format_with_host(&PathBuf::from("./test.txt_ps"), file_text.replace("plugin: ", ""), &HashMap::new())
        } else if file_text.starts_with("plugin-config: ") {
            let mut config_map = HashMap::new();
            config_map.insert("ending".to_string(), "custom_config".into());
            format_with_host(&PathBuf::from("./test.txt_ps"), file_text.replace("plugin-config: ", ""), &config_map)
        } else if file_text == "should_error" {
            err!("Did error.")
        } else if file_text == "should_panic" {
            self.has_panicked = true;
            panic!("Test panic")
        } else if file_text.ends_with(&config.ending) {
            Ok(String::from(file_text))
        } else {
            Ok(format!("{}_{}", file_text, config.ending))
        }
    }
}

generate_plugin_code!(TestWasmPlugin, TestWasmPlugin::new());
