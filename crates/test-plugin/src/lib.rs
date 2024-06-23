use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::get_nullable_vec;
use dprint_core::configuration::get_unknown_property_diagnostics;
use dprint_core::configuration::get_value;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::generate_plugin_code;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::ConfigChangeKind;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::PluginResolveConfigurationResult;
use dprint_core::plugins::SyncFormatRequest;
use dprint_core::plugins::SyncHostFormatRequest;
use dprint_core::plugins::SyncPluginHandler;
use serde::Deserialize;
use serde::Serialize;
use std::io::Write;
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
  fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> PluginResolveConfigurationResult<Configuration> {
    fn get_string_vec(config: &mut ConfigKeyMap, key: &str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> Option<Vec<String>> {
      get_nullable_vec(
        config,
        key,
        |value, _index, diagnostics| match value {
          ConfigKeyValue::String(value) => Some(value),
          _ => {
            diagnostics.push(ConfigurationDiagnostic {
              property_name: key.to_string(),
              message: "Expected only string values.".to_string(),
            });
            None
          }
        },
        diagnostics,
      )
    }

    let mut config = config;
    let mut diagnostics = Vec::new();
    let ending = get_value(&mut config, "ending", String::from("formatted"), &mut diagnostics);
    let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

    let file_extensions = get_string_vec(&mut config, "file_extensions", &mut diagnostics).unwrap_or_else(|| vec!["txt".to_string()]);
    let file_names = get_string_vec(&mut config, "file_names", &mut diagnostics).unwrap_or_else(|| vec![]);

    diagnostics.extend(get_unknown_property_diagnostics(config));

    PluginResolveConfigurationResult {
      config: Configuration { ending, line_width },
      diagnostics,
      file_matching: FileMatchingInfo { file_extensions, file_names },
    }
  }

  fn plugin_info(&mut self) -> PluginInfo {
    PluginInfo {
      name: env!("CARGO_PKG_NAME").to_string(),
      version: env!("CARGO_PKG_VERSION").to_string(),
      config_key: "test-plugin".to_string(),
      help_url: "https://dprint.dev/plugins/test".to_string(),
      config_schema_url: "https://plugins.dprint.dev/test/schema.json".to_string(),
      update_url: Some("https://plugins.dprint.dev/dprint/test-plugin/latest.json".to_string()),
    }
  }

  fn license_text(&mut self) -> String {
    std::str::from_utf8(include_bytes!("../LICENSE")).unwrap().into()
  }

  fn check_config_updates(&self, message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    let mut changes = Vec::new();
    if message.config.contains_key("should_add") {
      changes.extend([
        ConfigChange {
          path: vec!["should_add".to_string().into()],
          kind: ConfigChangeKind::Set(ConfigKeyValue::String("new_value_wasm".to_string())),
        },
        ConfigChange {
          path: vec!["new_prop1".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Array(vec![ConfigKeyValue::String("new_value_wasm".to_string())])),
        },
        ConfigChange {
          path: vec!["new_prop2".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Object(ConfigKeyMap::from([(
            "new_prop".to_string(),
            ConfigKeyValue::String("new_value_wasm".to_string()),
          )]))),
        },
      ]);
    }
    if message.config.contains_key("should_set") {
      changes.push(ConfigChange {
        path: vec!["should_set".to_string().into()],
        kind: ConfigChangeKind::Set(ConfigKeyValue::String("new_value_wasm".to_string())),
      });
    }
    if message.config.contains_key("should_remove") {
      changes.push(ConfigChange {
        path: vec!["should_remove".to_string().into()],
        kind: ConfigChangeKind::Remove,
      });
    }
    if message.config.contains_key("should_set_past_version") {
      changes.push(ConfigChange {
        path: vec!["should_set_past_version".to_string().into()],
        kind: ConfigChangeKind::Set(ConfigKeyValue::String(message.old_version.unwrap())),
      });
    }
    Ok(changes)
  }

  fn format(&mut self, request: SyncFormatRequest<Configuration>, mut format_with_host: impl FnMut(SyncHostFormatRequest) -> FormatResult) -> FormatResult {
    fn handle_host_response(result: FormatResult, original_text: &str) -> Result<String> {
      match result {
        Ok(Some(text)) => Ok(String::from_utf8(text).unwrap()),
        Ok(None) => Ok(original_text.to_string()),
        Err(err) => Err(err),
      }
    }

    let file_text = String::from_utf8(request.file_bytes).unwrap();
    if file_text == "wait_cancellation" {
      loop {
        if request.token.is_cancelled() {
          return Ok(None);
        }
      }
    }
    if let Some(output) = file_text.strip_prefix("stderr:") {
      let mut stderr = dprint_core::plugins::wasm::WasiPrintFd(2);
      stderr.write_all(output.as_bytes()).unwrap();
    } else if let Some(output) = file_text.strip_prefix("stdout:") {
      let mut stderr = dprint_core::plugins::wasm::WasiPrintFd(1);
      stderr.write_all(output.as_bytes()).unwrap();
    }

    let (had_suffix, file_text) = if let Some(text) = file_text.strip_suffix(&format!("_{}", request.config.ending)) {
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
          format_with_host(SyncHostFormatRequest {
            file_path: &PathBuf::from("./test.txt_ps"),
            file_bytes: new_text.as_bytes(),
            range: None,
            override_config: &ConfigKeyMap::new(),
          }),
          new_text,
        )?,
      )
    } else if let Some(new_text) = file_text.strip_prefix("plugin-config: ") {
      let mut config_map = ConfigKeyMap::new();
      config_map.insert("ending".to_string(), "custom_config".into());
      format!(
        "plugin-config: {}",
        handle_host_response(
          format_with_host(SyncHostFormatRequest {
            file_path: &PathBuf::from("./test.txt_ps"),
            file_bytes: new_text.as_bytes(),
            range: None,
            override_config: &config_map,
          }),
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
    } else if file_text.starts_with("format_to_empty") {
      return Ok(Some(String::new().into_bytes()));
    } else {
      file_text.to_string()
    };

    if had_suffix && inner_format_text == file_text {
      Ok(None)
    } else {
      Ok(Some(format!("{}_{}", inner_format_text, request.config.ending).into_bytes()))
    }
  }
}

generate_plugin_code!(TestWasmPlugin, TestWasmPlugin::new());
