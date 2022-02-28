use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::FormatRequest;
use dprint_core::plugins::Host;
use serde::Deserialize;
use serde::Serialize;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

use dprint_core::configuration::get_unknown_property_diagnostics;
use dprint_core::configuration::get_value;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::configuration::ResolveConfigurationResult;
use dprint_core::plugins::process::get_parent_process_id_from_cli_args;
use dprint_core::plugins::process::handle_process_stdio_messages;
use dprint_core::plugins::process::start_parent_process_checker_task;
use dprint_core::plugins::PluginHandler;
use dprint_core::plugins::PluginInfo;

#[tokio::main]
async fn main() -> Result<()> {
  if let Some(parent_process_id) = get_parent_process_id_from_cli_args() {
    start_parent_process_checker_task(parent_process_id);
  }

  // needs to run on a blocking task
  tokio::task::spawn_blocking(|| handle_process_stdio_messages(TestProcessPluginHandler::new()))
    .await
    .unwrap()
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

impl PluginHandler for TestProcessPluginHandler {
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
      supports_range_format: true,
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

  fn format<TCancellationToken: CancellationToken>(
    &self,
    request: FormatRequest<Self::Configuration, TCancellationToken>,
    host: impl Host,
  ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>> {
    Box::pin(async move {
      if request.file_text.starts_with("plugin: ") {
        host
          .format(PathBuf::from("./test.txt"), request.file_text.replace("plugin: ", ""), None, None)
          .await
      } else if request.file_text.starts_with("plugin-config: ") {
        let mut config_map = ConfigKeyMap::new();
        config_map.insert("ending".to_string(), "custom_config".into());
        host
          .format(
            PathBuf::from("./test.txt"),
            request.file_text.replace("plugin-config: ", ""),
            None,
            Some(&config_map),
          )
          .await
      } else if request.file_text == "should_error" {
        bail!("Did error.")
      } else if request.file_text.ends_with(&request.config.ending) {
        Ok(None)
      } else {
        Ok(Some(format!("{}_{}", request.file_text, request.config.ending)))
      }
    })
  }
}
