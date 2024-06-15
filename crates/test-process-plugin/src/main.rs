use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::configuration::get_nullable_vec;
use dprint_core::configuration::get_unknown_property_diagnostics;
use dprint_core::configuration::get_value;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::process::get_parent_process_id_from_cli_args;
use dprint_core::plugins::process::handle_process_stdio_messages;
use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::AsyncPluginHandler;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::ConfigChangeKind;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatRequest;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::PluginResolveConfigurationResult;
use serde::Deserialize;
use serde::Serialize;

fn main() -> Result<()> {
  let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
  rt.block_on(async move {
    if let Some(parent_process_id) = get_parent_process_id_from_cli_args() {
      start_parent_process_checker_task(parent_process_id);
    }

    handle_process_stdio_messages(TestProcessPluginHandler::new()).await
  })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
  ending: String,
  line_width: u32,
}

struct TestProcessPluginHandler {}

impl TestProcessPluginHandler {
  fn new() -> Self {
    TestProcessPluginHandler {}
  }
}

#[async_trait(?Send)]
impl AsyncPluginHandler for TestProcessPluginHandler {
  type Configuration = Configuration;

  fn plugin_info(&self) -> PluginInfo {
    PluginInfo {
      name: String::from(env!("CARGO_PKG_NAME")),
      version: String::from(env!("CARGO_PKG_VERSION")),
      config_key: "testProcessPlugin".to_string(),
      help_url: "https://dprint.dev/plugins/test-process".to_string(),
      config_schema_url: "".to_string(),
      update_url: Some("https://plugins.dprint.dev/dprint/test-process-plugin/latest.json".to_string()),
    }
  }

  fn license_text(&self) -> String {
    "License text.".to_string()
  }

  async fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> PluginResolveConfigurationResult<Configuration> {
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
    let ending = get_value(&mut config, "ending", String::from("formatted_process"), &mut diagnostics);
    let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

    let file_extensions = get_string_vec(&mut config, "file_extensions", &mut diagnostics).unwrap_or_else(|| vec!["txt_ps".to_string()]);
    let file_names = get_string_vec(&mut config, "file_names", &mut diagnostics).unwrap_or_else(|| vec!["test-process-plugin-exact-file".to_string()]);

    diagnostics.extend(get_unknown_property_diagnostics(config));

    PluginResolveConfigurationResult {
      file_matching: FileMatchingInfo { file_extensions, file_names },
      config: Configuration { ending, line_width },
      diagnostics,
    }
  }

  async fn check_config_updates(&self, config: ConfigKeyMap) -> Result<Vec<ConfigChange>> {
    let mut changes = Vec::new();
    if config.contains_key("should_add") {
      changes.extend([
        ConfigChange {
          path: vec!["should_add".to_string().into()],
          kind: ConfigChangeKind::Set(ConfigKeyValue::String("new_value".to_string())),
        },
        ConfigChange {
          path: vec!["new_prop1".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Array(vec![ConfigKeyValue::String("new_value".to_string())])),
        },
        ConfigChange {
          path: vec!["new_prop2".to_string().into()],
          kind: ConfigChangeKind::Add(ConfigKeyValue::Object(ConfigKeyMap::from([(
            "new_prop".to_string(),
            ConfigKeyValue::String("new_value".to_string()),
          )]))),
        },
      ]);
    }
    if config.contains_key("should_set") {
      changes.push(ConfigChange {
        path: vec!["should_set".to_string().into()],
        kind: ConfigChangeKind::Set(ConfigKeyValue::String("new_value".to_string())),
      });
    }
    if config.contains_key("should_remove") {
      changes.push(ConfigChange {
        path: vec!["should_remove".to_string().into()],
        kind: ConfigChangeKind::Remove,
      });
    }
    Ok(changes)
  }

  async fn format(
    &self,
    request: FormatRequest<Self::Configuration>,
    mut format_with_host: impl FnMut(HostFormatRequest) -> LocalBoxFuture<'static, FormatResult> + 'static,
  ) -> FormatResult {
    let file_text = String::from_utf8(request.file_bytes)?;
    let (had_suffix, file_text) = if let Some(text) = file_text.strip_suffix(&format!("_{}", request.config.ending)) {
      (true, text.to_string())
    } else {
      (false, file_text.to_string())
    };

    let inner_format_text = if let Some(range) = &request.range {
      let text = format!("{}_{}_{}", &file_text[0..range.start], request.config.ending, &file_text[range.end..]);
      text
    } else if file_text.starts_with("wait_cancellation") {
      request.token.wait_cancellation().await;
      return Ok(None);
    } else if let Some(new_text) = file_text.strip_prefix("plugin: ") {
      let result = (format_with_host)(HostFormatRequest {
        file_path: PathBuf::from("./test.txt"),
        file_bytes: new_text.to_string().into_bytes(),
        range: None,
        override_config: Default::default(),
        token: request.token.clone(),
      })
      .await?;
      format!(
        "plugin: {}",
        result.map(|r| String::from_utf8(r).unwrap()).unwrap_or_else(|| new_text.to_string())
      )
    } else if let Some(new_text) = file_text.strip_prefix("plugin-config: ") {
      let mut config_map = ConfigKeyMap::new();
      config_map.insert("ending".to_string(), "custom_config".into());
      let result = (format_with_host)(HostFormatRequest {
        file_path: PathBuf::from("./test.txt"),
        file_bytes: new_text.to_string().into_bytes(),
        range: None,
        override_config: config_map,
        token: request.token.clone(),
      })
      .await?;
      format!(
        "plugin-config: {}",
        result.map(|r| String::from_utf8(r).unwrap()).unwrap_or_else(|| new_text.to_string())
      )
    } else if file_text == "should_error" {
      bail!("Did error.")
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
