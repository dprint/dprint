use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use dprint_core::plugins::PluginInfo;

use crate::environment::Environment;

const PLUGIN_SCHEMA_VERSION: usize = 4;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheManifest {
  schema_version: usize,
  plugins: HashMap<String, PluginCacheManifestItem>,
}

impl PluginCacheManifest {
  pub(super) fn new() -> PluginCacheManifest {
    PluginCacheManifest {
      schema_version: PLUGIN_SCHEMA_VERSION,
      plugins: HashMap::new(),
    }
  }

  pub fn add_item(&mut self, key: String, item: PluginCacheManifestItem) {
    self.plugins.insert(key, item);
  }

  pub fn get_item(&self, key: &str) -> Option<&PluginCacheManifestItem> {
    self.plugins.get(key)
  }

  pub fn remove_item(&mut self, key: &str) -> Option<PluginCacheManifestItem> {
    self.plugins.remove(key)
  }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheManifestItem {
  /// Created time in *seconds* since epoch.
  pub created_time: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub file_hash: Option<u64>,
  pub info: PluginInfo,
}

pub fn read_manifest(environment: &impl Environment) -> PluginCacheManifest {
  return match try_deserialize(environment) {
    Ok(manifest) => {
      if manifest.schema_version != PLUGIN_SCHEMA_VERSION {
        let _ = environment.remove_dir_all(&environment.get_cache_dir().join("plugins"));
        PluginCacheManifest::new()
      } else {
        manifest
      }
    }
    Err(_) => {
      let _ = environment.remove_dir_all(&environment.get_cache_dir());
      PluginCacheManifest::new()
    }
  };

  fn try_deserialize(environment: &impl Environment) -> Result<PluginCacheManifest> {
    let file_path = get_manifest_file_path(environment);
    match environment.read_file(&file_path) {
      Ok(text) => Ok(serde_json::from_str::<PluginCacheManifest>(&text)?),
      Err(_) => Ok(PluginCacheManifest::new()),
    }
  }
}

pub fn write_manifest(manifest: &PluginCacheManifest, environment: &impl Environment) -> Result<()> {
  let file_path = get_manifest_file_path(environment);
  let serialized_manifest = serde_json::to_string(&manifest)?;
  environment.write_file(&file_path, &serialized_manifest)
}

fn get_manifest_file_path(environment: &impl Environment) -> PathBuf {
  let cache_dir = environment.get_cache_dir();
  cache_dir.join("plugin-cache-manifest.json")
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn should_read_ok_manifest() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &environment.get_cache_dir().join("plugin-cache-manifest.json"),
        r#"{
    "schemaVersion": 4,
    "plugins": {
        "a": {
            "createdTime": 123,
            "info": {
                "name": "dprint-plugin-typescript",
                "version": "0.1.0",
                "configKey": "typescript",
                "fileExtensions": [".ts"],
                "helpUrl": "help url",
                "configSchemaUrl": "schema url"
            }
        },
        "c": {
            "createdTime": 456,
            "fileHash": 10,
            "info": {
                "name": "dprint-plugin-json",
                "version": "0.2.0",
                "configKey": "json",
                "fileExtensions": [".json"],
                "helpUrl": "help url 2",
                "configSchemaUrl": "schema url 2"
            }
        },
        "cargo": {
            "createdTime": 210530,
            "fileHash": 1226,
            "info": {
                "name": "dprint-plugin-cargo",
                "version": "0.2.1",
                "configKey": "cargo",
                "fileExtensions": [],
                "fileNames": ["Cargo.toml"],
                "helpUrl": "cargo help url",
                "configSchemaUrl": "cargo schema url"
            }
        }
    }
}"#,
      )
      .unwrap();

    let mut expected_manifest = PluginCacheManifest::new();
    expected_manifest.add_item(
      String::from("a"),
      PluginCacheManifestItem {
        created_time: 123,
        file_hash: None,
        info: PluginInfo {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.1.0".to_string(),
          config_key: "typescript".to_string(),
          file_extensions: vec![".ts".to_string()],
          file_names: vec![],
          help_url: "help url".to_string(),
          config_schema_url: "schema url".to_string(),
        },
      },
    );
    expected_manifest.add_item(
      String::from("c"),
      PluginCacheManifestItem {
        created_time: 456,
        file_hash: Some(10),
        info: PluginInfo {
          name: "dprint-plugin-json".to_string(),
          version: "0.2.0".to_string(),
          config_key: "json".to_string(),
          file_extensions: vec![".json".to_string()],
          file_names: vec![],
          help_url: "help url 2".to_string(),
          config_schema_url: "schema url 2".to_string(),
        },
      },
    );
    expected_manifest.add_item(
      String::from("cargo"),
      PluginCacheManifestItem {
        created_time: 210530,
        file_hash: Some(1226),
        info: PluginInfo {
          name: "dprint-plugin-cargo".to_string(),
          version: "0.2.1".to_string(),
          config_key: "cargo".to_string(),
          file_extensions: vec![],
          file_names: vec!["Cargo.toml".to_string()],
          help_url: "cargo help url".to_string(),
          config_schema_url: "cargo schema url".to_string(),
        },
      },
    );

    assert_eq!(read_manifest(&environment), expected_manifest);
  }

  #[test]
  fn should_not_error_for_old_manifest() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &environment.get_cache_dir().join("plugin-cache-manifest.json"),
        r#"{
    "schemaVersion": 1,
    "plugins": {
        "a": {
            "createdTime": 123,
            "info": {
                "name": "dprint-plugin-typescript",
                "version": "0.1.0",
                "configKey": "typescript",
                "fileExtensions": [".ts"],
                "helpUrl": "help url",
                "configSchemaUrl": "schema url"
            }
        }
    }
}"#,
      )
      .unwrap();

    let expected_manifest = PluginCacheManifest::new();
    assert_eq!(read_manifest(&environment), expected_manifest);
  }

  #[test]
  fn should_have_empty_manifest_for_deserialization_error() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &environment.get_cache_dir().join("plugin-cache-manifest.json"),
        r#"{ "plugins": { "a": { file_name: "b", } } }"#,
      )
      .unwrap();

    assert_eq!(read_manifest(&environment), PluginCacheManifest::new());
  }

  #[test]
  fn should_deal_with_non_existent_manifest() {
    let environment = TestEnvironment::new();

    assert_eq!(read_manifest(&environment), PluginCacheManifest::new());
    assert_eq!(environment.take_stderr_messages().len(), 0);
  }

  #[test]
  fn it_save_manifest() {
    let environment = TestEnvironment::new();
    let mut manifest = PluginCacheManifest::new();
    manifest.add_item(
      String::from("a"),
      PluginCacheManifestItem {
        created_time: 456,
        file_hash: Some(256),
        info: PluginInfo {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.1.0".to_string(),
          config_key: "typescript".to_string(),
          file_extensions: vec![".ts".to_string()],
          file_names: vec![],
          help_url: "help url".to_string(),
          config_schema_url: "schema url".to_string(),
        },
      },
    );
    manifest.add_item(
      String::from("b"),
      PluginCacheManifestItem {
        created_time: 456,
        file_hash: None,
        info: PluginInfo {
          name: "dprint-plugin-json".to_string(),
          version: "0.2.0".to_string(),
          config_key: "json".to_string(),
          file_extensions: vec![".json".to_string()],
          file_names: vec!["file.test".to_string()],
          help_url: "help url 2".to_string(),
          config_schema_url: "schema url 2".to_string(),
        },
      },
    );
    write_manifest(&manifest, &environment).unwrap();

    // Just read and compare again because the hash map will serialize properties
    // in a non-deterministic order.
    assert_eq!(read_manifest(&environment), manifest);
  }
}
