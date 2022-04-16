use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::BoxFuture;
use dprint_core::plugins::FormatRequest;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::Host;
use dprint_core::plugins::HostFormatRequest;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

use dprint_core::configuration::get_unknown_property_diagnostics;
use dprint_core::configuration::get_value;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::configuration::ResolveConfigurationResult;
use dprint_core::plugins::process::get_parent_process_id_from_cli_args;
use dprint_core::plugins::process::handle_process_stdio_messages;
use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::AsyncPluginHandler;
use dprint_core::plugins::PluginInfo;

#[tokio::main]
async fn main() -> Result<()> {
  if let Some(parent_process_id) = get_parent_process_id_from_cli_args() {
    start_parent_process_checker_task(parent_process_id);
  }

  handle_process_stdio_messages(TestProcessPluginHandler::new()).await
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

impl AsyncPluginHandler for TestProcessPluginHandler {
  type Configuration = Configuration;

  fn plugin_info(&self) -> PluginInfo {
    PluginInfo {
      name: String::from(env!("CARGO_PKG_NAME")),
      version: String::from(env!("CARGO_PKG_VERSION")),
      config_key: "testProcessPlugin".to_string(),
      file_extensions: vec!["txt_ps".to_string()],
      file_names: vec!["test-process-plugin-exact-file".to_string()],
      help_url: "https://dprint.dev/plugins/test-process".to_string(),
      config_schema_url: "".to_string(),
      update_url: None,
    }
  }

  fn license_text(&self) -> String {
    "License text.".to_string()
  }

  fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
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

  fn format(&self, request: FormatRequest<Self::Configuration>, host: Arc<dyn Host>) -> BoxFuture<FormatResult> {
    Box::pin(async move {
      let (had_suffix, file_text) = if let Some(text) = request.file_text.strip_suffix(&format!("_{}", request.config.ending)) {
        (true, text.to_string())
      } else {
        (false, request.file_text.to_string())
      };

      let inner_format_text = if file_text.starts_with("wait_cancellation") {
      if let Some(range) = &request.range {
        let text = format!(
          "{}_{}_{}",
          &request.file_text[0..range.start],
          request.config.ending,
          &request.file_text[range.end..]
        );
        Ok(Some(text))
      } else if request.file_text.starts_with("wait_cancellation") {
        request.token.wait_cancellation().await;
        return Ok(None);
      } else if let Some(new_text) = file_text.strip_prefix("plugin: ") {
        let result = host
          .format(HostFormatRequest {
            file_path: PathBuf::from("./test.txt"),
            file_text: new_text.to_string(),
            range: None,
            override_config: Default::default(),
            token: request.token.clone(),
          })
          .await?;
        format!("plugin: {}", result.unwrap_or_else(|| new_text.to_string()))
      } else if let Some(new_text) = file_text.strip_prefix("plugin-config: ") {
        let mut config_map = ConfigKeyMap::new();
        config_map.insert("ending".to_string(), "custom_config".into());
        let result = host
          .format(HostFormatRequest {
            file_path: PathBuf::from("./test.txt"),
            file_text: new_text.to_string(),
            range: None,
            override_config: config_map,
            token: request.token.clone(),
          })
          .await?;
        format!("plugin-config: {}", result.unwrap_or_else(|| new_text.to_string()))
      } else if file_text == "should_error" {
        bail!("Did error.")
      } else {
        file_text.to_string()
      };

      if had_suffix && inner_format_text == file_text {
        Ok(None)
      } else {
        Ok(Some(format!("{}_{}", inner_format_text, request.config.ending)))
      }
    })
  }
}
