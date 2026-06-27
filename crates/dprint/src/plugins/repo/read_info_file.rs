use anyhow::Result;
use anyhow::bail;
use jsonc_parser::JsonArray;
use jsonc_parser::JsonObject;
use jsonc_parser::JsonValue;
use jsonc_parser::parse_to_value;
use url::Url;

use crate::environment::Environment;

// note: these don't derive `Eq` because `serde_json::Value` isn't `Eq`

#[derive(PartialEq, Debug)]
pub struct InfoFile {
  pub plugin_system_schema_version: u32,
  pub latest_plugins: Vec<InfoFilePluginInfo>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct InfoFilePluginInfo {
  pub name: String,
  pub version: String,
  pub url: String,
  pub config_key: Option<String>,
  pub file_extensions: Vec<String>,
  pub file_names: Vec<String>,
  pub config_excludes: Vec<String>,
  pub checksum: Option<String>,
  /// Config to insert into the plugin's config block on `dprint init`.
  pub default_config: Option<serde_json::Value>,
  /// Config fragments that `dprint init` merges into the plugin's config block
  /// when their files are found in the current directory (ex. wiring up a
  /// `dprint-plugin-exec` command for a matched file type).
  pub config_items: Vec<InfoFileConfigItem>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct InfoFileConfigItem {
  pub file_extensions: Vec<String>,
  pub file_names: Vec<String>,
  pub config: serde_json::Value,
}

impl InfoFilePluginInfo {
  pub fn is_wasm(&self) -> bool {
    self.url.to_lowercase().ends_with(".wasm")
  }

  pub fn is_process_plugin(&self) -> bool {
    !self.is_wasm()
  }

  pub fn full_url(&self) -> String {
    if let Some(checksum) = &self.checksum {
      return format!("{}@{}", self.url, checksum);
    }
    self.url.to_string()
  }

  pub fn full_url_no_wasm_checksum(&self) -> String {
    if self.is_wasm() { self.url.to_string() } else { self.full_url() }
  }
}

const SCHEMA_VERSION: u8 = 4;
pub const REMOTE_INFO_URL: &str = "https://plugins.dprint.dev/info.json";

pub async fn read_info_file(environment: &impl Environment) -> Result<InfoFile> {
  let (_, info_file) = environment.download_file_err_404(&Url::parse(REMOTE_INFO_URL)?, None).await?;
  let info_text = String::from_utf8(info_file.content)?;
  let json_value = parse_to_value(&info_text, &Default::default())?;
  let mut obj = match json_value {
    Some(JsonValue::Object(obj)) => obj,
    _ => bail!("Expected object in root element."),
  };

  // check schema version
  let schema_version = match obj.take_number("schemaVersion") {
    Some(value) => value.parse::<u32>()?,
    _ => bail!("Could not find schema version."),
  };
  if schema_version != SCHEMA_VERSION as u32 {
    bail!(
      "Cannot handle schema version {}. Expected {}. This might mean your dprint CLI version is old and isn't able to get the latest information.",
      schema_version,
      SCHEMA_VERSION
    );
  }

  // get plugin system version
  let plugin_system_schema_version = match obj.take_number("pluginSystemSchemaVersion") {
    Some(value) => value.parse::<u32>()?,
    _ => bail!("Could not find plugin system schema version."),
  };

  let latest_plugins = match obj.take_array("latest") {
    Some(arr) => {
      let mut plugins = Vec::new();
      for value in arr.into_iter() {
        plugins.push(get_latest_plugin(value)?);
      }
      plugins
    }
    _ => bail!("Could not find latest plugins array."),
  };

  Ok(InfoFile {
    plugin_system_schema_version,
    latest_plugins,
  })
}

fn get_latest_plugin(value: JsonValue) -> Result<InfoFilePluginInfo> {
  let mut obj = match value {
    JsonValue::Object(obj) => obj,
    _ => bail!("Expected an object in the latest array."),
  };
  let name = get_string(&mut obj, "name")?;
  let version = get_string(&mut obj, "version")?;
  let url = get_string(&mut obj, "url")?;
  let config_key = obj.take_string("configKey").map(|k| k.into_owned());
  let file_extensions = get_string_array(&mut obj, "fileExtensions")?;
  let file_names = get_string_array(&mut obj, "fileNames").unwrap_or_default(); // compatible with old configuration
  let config_excludes = get_string_array(&mut obj, "configExcludes")?;
  let checksum = obj.take_string("checksum").map(|s| s.into_owned());
  // these are only used by `dprint init`, so parse them leniently rather than
  // failing the whole info file when a single entry is malformed
  let default_config = obj.take_object("defaultConfig").map(|o| jsonc_to_serde(JsonValue::Object(o)));
  let config_items = obj.take_array("configItems").map(parse_config_items).unwrap_or_default();

  Ok(InfoFilePluginInfo {
    name,
    version,
    url,
    config_key,
    file_extensions,
    file_names,
    config_excludes,
    checksum,
    default_config,
    config_items,
  })
}

fn parse_config_items(arr: JsonArray) -> Vec<InfoFileConfigItem> {
  let mut items = Vec::new();
  for value in arr.into_iter() {
    let JsonValue::Object(mut obj) = value else {
      continue;
    };
    let (file_extensions, file_names) = match obj.take_object("match") {
      Some(mut match_obj) => (
        take_string_array(&mut match_obj, "fileExtensions"),
        take_string_array(&mut match_obj, "fileNames"),
      ),
      None => (Vec::new(), Vec::new()),
    };
    let config = obj
      .take_object("config")
      .map(|o| jsonc_to_serde(JsonValue::Object(o)))
      .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    items.push(InfoFileConfigItem {
      file_extensions,
      file_names,
      config,
    });
  }
  items
}

/// Converts a parsed jsonc value into an owned `serde_json::Value`.
fn jsonc_to_serde(value: JsonValue) -> serde_json::Value {
  use serde_json::Value as Json;
  match value {
    JsonValue::Null => Json::Null,
    JsonValue::Boolean(value) => Json::Bool(value),
    JsonValue::Number(value) => serde_json::from_str(value).unwrap_or(Json::Null),
    JsonValue::String(value) => Json::String(value.into_owned()),
    JsonValue::Array(arr) => Json::Array(arr.into_iter().map(jsonc_to_serde).collect()),
    JsonValue::Object(obj) => Json::Object(obj.into_iter().map(|(key, value)| (key, jsonc_to_serde(value))).collect()),
  }
}

/// Gets a string array, ignoring the key when it's missing or any non-string entries.
fn take_string_array(obj: &mut JsonObject, key: &str) -> Vec<String> {
  match obj.take_array(key) {
    Some(arr) => arr
      .into_iter()
      .filter_map(|value| match value {
        JsonValue::String(value) => Some(value.into_owned()),
        _ => None,
      })
      .collect(),
    None => Vec::new(),
  }
}

fn get_string_array(value: &mut JsonObject, key: &str) -> Result<Vec<String>> {
  let mut result = Vec::new();
  for item in get_array(value, key)? {
    match item {
      JsonValue::String(item) => result.push(item.into_owned()),
      _ => bail!("Unexpected non-string in {} array.", key),
    }
  }
  Ok(result)
}

fn get_string(value: &mut JsonObject, name: &str) -> Result<String> {
  match value.take_string(name) {
    Some(text) => Ok(text.into_owned()),
    _ => bail!("Could not find string: {}", name),
  }
}

fn get_array<'a>(value: &mut JsonObject<'a>, name: &str) -> Result<JsonArray<'a>> {
  match value.take_array(name) {
    Some(arr) => Ok(arr),
    _ => bail!("Could not find array: {}", name),
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFileConfigItem;
  use crate::environment::TestInfoFileMatch;
  use crate::environment::TestInfoFilePlugin;
  use pretty_assertions::assert_eq;

  #[test]
  fn should_get_info() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-typescript".to_string(),
            version: "0.17.2".to_string(),
            url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
            config_key: Some("typescript".to_string()),
            file_extensions: vec!["ts".to_string(), "tsx".to_string()],
            config_excludes: vec!["**/node_modules".to_string()],
            ..Default::default()
          })
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-jsonc".to_string(),
            version: "0.2.3".to_string(),
            url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
            config_key: None,
            file_extensions: vec!["json".to_string()],
            file_names: Some(vec!["test-file".to_string()]),
            config_excludes: vec!["**/*-lock.json".to_string()],
            checksum: Some("test-checksum".to_string()),
            ..Default::default()
          });
      })
      .build();
    environment.clone().run_in_runtime(async move {
      let info_file = read_info_file(&environment).await.unwrap();
      assert_eq!(
        info_file,
        InfoFile {
          plugin_system_schema_version: 4,
          latest_plugins: vec![
            InfoFilePluginInfo {
              name: "dprint-plugin-typescript".to_string(),
              version: "0.17.2".to_string(),
              url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
              config_key: Some("typescript".to_string()),
              file_extensions: vec!["ts".to_string(), "tsx".to_string()],
              file_names: vec![],
              config_excludes: vec!["**/node_modules".to_string()],
              checksum: None,
              default_config: None,
              config_items: vec![],
            },
            InfoFilePluginInfo {
              name: "dprint-plugin-jsonc".to_string(),
              version: "0.2.3".to_string(),
              url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
              config_key: None,
              file_extensions: vec!["json".to_string()],
              file_names: vec!["test-file".to_string()],
              config_excludes: vec!["**/*-lock.json".to_string()],
              checksum: Some("test-checksum".to_string()),
              default_config: None,
              config_items: vec![],
            }
          ],
        }
      )
    });
  }

  #[test]
  fn should_parse_default_config_and_config_items() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-exec".to_string(),
          version: "0.5.0".to_string(),
          url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
          config_key: Some("exec".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          checksum: Some("checksum".to_string()),
          default_config: Some(serde_json::json!({ "cwd": "${configDir}" })),
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec!["rs".to_string()],
              file_names: vec![],
            },
            config: serde_json::json!({ "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
          }],
          ..Default::default()
        });
      })
      .build();
    environment.clone().run_in_runtime(async move {
      let info_file = read_info_file(&environment).await.unwrap();
      let plugin = &info_file.latest_plugins[0];
      assert_eq!(plugin.default_config, Some(serde_json::json!({ "cwd": "${configDir}" })));
      assert_eq!(
        plugin.config_items,
        vec![InfoFileConfigItem {
          file_extensions: vec!["rs".to_string()],
          file_names: vec![],
          config: serde_json::json!({ "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
        }]
      );
    });
  }

  #[test]
  fn should_parse_init_fields_leniently() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      REMOTE_INFO_URL,
      r#"{
  "schemaVersion": 4,
  "pluginSystemSchemaVersion": 4,
  "latest": [{
    "name": "p",
    "version": "1.0.0",
    "url": "https://plugins.dprint.dev/p.wasm",
    "fileExtensions": ["x"],
    "configExcludes": [],
    "defaultConfig": "not-an-object",
    "configItems": [
      "not-an-object",
      { "config": { "a": 1 } },
      { "match": { "fileExtensions": ["y", 5] }, "config": { "b": 2 } }
    ]
  }]
}"#
        .as_bytes(),
    );
    environment.clone().run_in_runtime(async move {
      let info_file = read_info_file(&environment).await.unwrap();
      let plugin = &info_file.latest_plugins[0];
      // a non-object defaultConfig is ignored rather than failing the whole info file
      assert_eq!(plugin.default_config, None);
      assert_eq!(
        plugin.config_items,
        vec![
          // the bare string entry is skipped; a missing `match` defaults to no matchers
          InfoFileConfigItem {
            file_extensions: vec![],
            file_names: vec![],
            config: serde_json::json!({ "a": 1 }),
          },
          // the non-string extension is dropped
          InfoFileConfigItem {
            file_extensions: vec!["y".to_string()],
            file_names: vec![],
            config: serde_json::json!({ "b": 2 }),
          },
        ]
      );
    });
  }

  #[test]
  fn should_error_if_schema_version_is_different() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      REMOTE_INFO_URL,
      r#"{
    "schemaVersion": 1,
}"#
        .as_bytes(),
    );
    environment.clone().run_in_runtime(async move {
      let message = read_info_file(&environment).await.err().unwrap();
      assert_eq!(
        message.to_string(),
        "Cannot handle schema version 1. Expected 4. This might mean your dprint CLI version is old and isn't able to get the latest information."
      );
    });
  }

  #[test]
  fn should_error_if_no_plugin_system_set() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(
      REMOTE_INFO_URL,
      r#"{
    "schemaVersion": 4,
}"#
        .as_bytes(),
    );
    environment.clone().run_in_runtime(async move {
      let message = read_info_file(&environment).await.err().unwrap();
      assert_eq!(message.to_string(), "Could not find plugin system schema version.");
    });
  }

  #[test]
  fn should_error_when_info_file_not_exists() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let message = read_info_file(&environment).await.err().unwrap();
      assert_eq!(message.to_string(), "Error downloading https://plugins.dprint.dev/info.json - 404 Not Found");
    });
  }

  #[test]
  fn should_error_when_info_file_errors() {
    let environment = TestEnvironment::new();
    environment.add_remote_file_error("https://plugins.dprint.dev/info.json", "Some Error");
    environment.clone().run_in_runtime(async move {
      let message = read_info_file(&environment).await.err().unwrap();
      assert_eq!(message.to_string(), "Some Error");
    });
  }
}
