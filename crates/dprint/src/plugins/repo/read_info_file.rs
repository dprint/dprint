use anyhow::bail;
use anyhow::Result;
use jsonc_parser::parse_to_value;
use jsonc_parser::JsonArray;
use jsonc_parser::JsonObject;
use jsonc_parser::JsonValue;

use crate::environment::Environment;

#[derive(PartialEq, Debug)]
pub struct InfoFile {
  pub plugin_system_schema_version: u32,
  pub latest_plugins: Vec<InfoFilePluginInfo>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct InfoFilePluginInfo {
  pub name: String,
  pub version: String,
  pub selected: bool,
  pub url: String,
  pub config_key: Option<String>,
  pub file_extensions: Vec<String>,
  pub file_names: Vec<String>,
  pub config_excludes: Vec<String>,
  pub checksum: Option<String>,
}

impl InfoFilePluginInfo {
  pub fn is_process_plugin(&self) -> bool {
    !self.url.to_lowercase().ends_with(".wasm")
  }
}

const SCHEMA_VERSION: u8 = 4;
pub const REMOTE_INFO_URL: &str = "https://plugins.dprint.dev/info.json";

pub fn read_info_file(environment: &impl Environment) -> Result<InfoFile> {
  let info_bytes = environment.download_file(REMOTE_INFO_URL)?;
  let info_text = String::from_utf8(info_bytes.to_vec())?;
  let json_value = parse_to_value(&info_text)?;
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
  let selected = obj.get_boolean("selected").unwrap_or(false);
  let config_key = obj.take_string("configKey").map(|k| k.into_owned());
  let file_extensions = get_string_array(&mut obj, "fileExtensions")?;
  let file_names = get_string_array(&mut obj, "fileNames").unwrap_or_default(); // compatible with old configuration
  let config_excludes = get_string_array(&mut obj, "configExcludes")?;
  let checksum = obj.take_string("checksum").map(|s| s.into_owned());

  Ok(InfoFilePluginInfo {
    name,
    version,
    url,
    selected,
    config_key,
    file_extensions,
    file_names,
    config_excludes,
    checksum,
  })
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
            selected: Some(true),
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
    let info_file = read_info_file(&environment).unwrap();
    assert_eq!(
      info_file,
      InfoFile {
        plugin_system_schema_version: 3,
        latest_plugins: vec![
          InfoFilePluginInfo {
            name: "dprint-plugin-typescript".to_string(),
            version: "0.17.2".to_string(),
            selected: true,
            url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
            config_key: Some("typescript".to_string()),
            file_extensions: vec!["ts".to_string(), "tsx".to_string()],
            file_names: vec![],
            config_excludes: vec!["**/node_modules".to_string()],
            checksum: None,
          },
          InfoFilePluginInfo {
            name: "dprint-plugin-jsonc".to_string(),
            version: "0.2.3".to_string(),
            selected: false,
            url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
            config_key: None,
            file_extensions: vec!["json".to_string()],
            file_names: vec!["test-file".to_string()],
            config_excludes: vec!["**/*-lock.json".to_string()],
            checksum: Some("test-checksum".to_string()),
          }
        ],
      }
    )
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
    let message = read_info_file(&environment).err().unwrap();
    assert_eq!(
      message.to_string(),
      "Cannot handle schema version 1. Expected 4. This might mean your dprint CLI version is old and isn't able to get the latest information."
    );
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
    let message = read_info_file(&environment).err().unwrap();
    assert_eq!(message.to_string(), "Could not find plugin system schema version.");
  }

  #[test]
  fn should_error_when_no_internet() {
    let environment = TestEnvironment::new();
    let message = read_info_file(&environment).err().unwrap();
    assert_eq!(message.to_string(), "Could not find file at url https://plugins.dprint.dev/info.json");
  }
}
