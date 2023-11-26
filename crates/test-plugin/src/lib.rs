use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::get_unknown_property_diagnostics;
use dprint_core::configuration::get_value;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::configuration::ResolveConfigurationResult;
use dprint_core::generate_plugin_code;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::SyncPluginHandler;
use dprint_core::plugins::SyncPluginInfo;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

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
    TestWasmPlugin { has_panicked: false }
  }
}

impl SyncPluginHandler<Configuration> for TestWasmPlugin {
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

  fn plugin_info(&mut self) -> SyncPluginInfo {
    SyncPluginInfo {
      info: PluginInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        config_key: "test-plugin".to_string(),
        help_url: "https://dprint.dev/plugins/test".to_string(),
        config_schema_url: "https://plugins.dprint.dev/test/schema.json".to_string(),
        update_url: Some("https://plugins.dprint.dev/dprint/test-plugin/latest.json".to_string()),
      },
      file_matching: FileMatchingInfo {
        file_extensions: vec!["txt".to_string()],
        file_names: vec![],
      },
    }
  }

  fn license_text(&mut self) -> String {
    std::str::from_utf8(include_bytes!("../LICENSE")).unwrap().into()
  }

  fn format(
    &mut self,
    _: &Path,
    file_bytes: Vec<u8>,
    config: &Configuration,
    mut format_with_host: impl FnMut(&Path, Vec<u8>, &ConfigKeyMap) -> FormatResult,
  ) -> FormatResult {
    fn handle_host_response(result: FormatResult, original_text: &str) -> Result<String> {
      match result {
        Ok(Some(text)) => Ok(String::from_utf8(text).unwrap()),
        Ok(None) => Ok(original_text.to_string()),
        Err(err) => Err(err),
      }
    }

    let file_text = String::from_utf8(file_bytes).unwrap();
    let (had_suffix, file_text) = if let Some(text) = file_text.strip_suffix(&format!("_{}", config.ending)) {
      (true, text.to_string())
    } else {
      (false, file_text)
    };

    let inner_format_text = if self.has_panicked {
      panic!("Previously panicked. Plugin should not have been used by the CLI again.")
    } else if let Some(new_text) = file_text.strip_prefix("plugin: ") {
      format!(
        "plugin: {}",
        handle_host_response(
          format_with_host(&PathBuf::from("./test.txt_ps"), new_text.to_string().into_bytes(), &ConfigKeyMap::new()),
          new_text,
        )?,
      )
    } else if let Some(new_text) = file_text.strip_prefix("plugin-config: ") {
      let mut config_map = ConfigKeyMap::new();
      config_map.insert("ending".to_string(), "custom_config".into());
      format!(
        "plugin-config: {}",
        handle_host_response(
          format_with_host(&PathBuf::from("./test.txt_ps"), new_text.to_string().into_bytes(), &config_map),
          new_text
        )?
      )
    } else if file_text == "should_error" {
      bail!("Did error.")
    } else if file_text == "should_panic" {
      self.has_panicked = true;
      panic!("Test panic")
    } else if file_text == "unstable_fmt_then_error" {
      "should_error".to_string()
    } else if file_text == "unstable_fmt_true" {
      "unstable_fmt_false".to_string()
    } else if file_text == "unstable_fmt_false" {
      "unstable_fmt_true".to_string()
    } else {
      file_text.to_string()
    };

    if had_suffix && inner_format_text == file_text {
      Ok(None)
    } else {
      Ok(Some(format!("{}_{}", inner_format_text, config.ending).into_bytes()))
    }
  }
}

generate_plugin_code!(TestWasmPlugin, TestWasmPlugin::new());
