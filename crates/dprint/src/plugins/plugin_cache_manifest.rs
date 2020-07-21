use dprint_core::plugins::PluginInfo;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use std::path::PathBuf;

use crate::environment::Environment;
use crate::types::ErrBox;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PluginCacheManifest(HashMap<String, PluginCacheManifestItem>);

impl PluginCacheManifest {
    pub(super) fn new() -> PluginCacheManifest {
        PluginCacheManifest(HashMap::new())
    }

    pub fn add_item(&mut self, key: String, item: PluginCacheManifestItem) {
        self.0.insert(key, item);
    }

    pub fn get_item(&self, key: &str) -> Option<&PluginCacheManifestItem> {
        self.0.get(key)
    }

    pub fn remove_item(&mut self, key: &str) -> Option<PluginCacheManifestItem> {
        self.0.remove(key)
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

pub fn read_manifest(environment: &impl Environment) -> Result<PluginCacheManifest, ErrBox> {
    let file_path = get_manifest_file_path(environment)?;
    match environment.read_file(&file_path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(manifest) => Ok(manifest),
            Err(err) => {
                environment.log_error(&format!("Error deserializing plugin cache manifest, but ignoring: {}", err));
                Ok(PluginCacheManifest::new())
            }
        },
        Err(_) => Ok(PluginCacheManifest::new()),
    }
}

pub fn write_manifest(manifest: &PluginCacheManifest, environment: &impl Environment) -> Result<(), ErrBox> {
    let file_path = get_manifest_file_path(environment)?;
    let serialized_manifest = serde_json::to_string(&manifest)?;
    environment.write_file(&file_path, &serialized_manifest)
}

fn get_manifest_file_path(environment: &impl Environment) -> Result<PathBuf, ErrBox> {
    let cache_dir = environment.get_cache_dir()?;
    Ok(cache_dir.join("plugin-cache-manifest.json"))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::environment::TestEnvironment;

    #[test]
    fn it_should_read_ok_manifest() {
        let environment = TestEnvironment::new();
        environment.write_file(
            &environment.get_cache_dir().unwrap().join("plugin-cache-manifest.json"),
            r#"{
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
    }
}"#
        ).unwrap();

        let mut expected_manifest = PluginCacheManifest::new();
        expected_manifest.add_item(String::from("a"), PluginCacheManifestItem {
            created_time: 123,
            file_hash: None,
            info: PluginInfo {
                name: "dprint-plugin-typescript".to_string(),
                version: "0.1.0".to_string(),
                config_key: "typescript".to_string(),
                file_extensions: vec![".ts".to_string()],
                help_url: "help url".to_string(),
                config_schema_url: "schema url".to_string()
            }
        });
        expected_manifest.add_item(String::from("c"), PluginCacheManifestItem {
            created_time: 456,
            file_hash: Some(10),
            info: PluginInfo {
                name: "dprint-plugin-json".to_string(),
                version: "0.2.0".to_string(),
                config_key: "json".to_string(),
                file_extensions: vec![".json".to_string()],
                help_url: "help url 2".to_string(),
                config_schema_url: "schema url 2".to_string()
            }
        });

        assert_eq!(read_manifest(&environment).unwrap(), expected_manifest);
    }

    #[test]
    fn it_should_have_empty_manifest_for_deserialization_error() {
        let environment = TestEnvironment::new();
        environment.write_file(
            &environment.get_cache_dir().unwrap().join("plugin-cache-manifest.json"),
            r#"{ "a": { file_name: "b", } }"#
        ).unwrap();

        assert_eq!(read_manifest(&environment).unwrap(), PluginCacheManifest::new());
        assert_eq!(environment.get_logged_errors(), vec![
            String::from("Error deserializing plugin cache manifest, but ignoring: key must be a string at line 1 column 10")
        ]);
    }

    #[test]
    fn it_should_deal_with_non_existent_manifest() {
        let environment = TestEnvironment::new();

        assert_eq!(read_manifest(&environment).unwrap(), PluginCacheManifest::new());
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_save_manifest() {
        let environment = TestEnvironment::new();
        let mut manifest = PluginCacheManifest::new();
        manifest.add_item(String::from("a"), PluginCacheManifestItem {
            created_time: 456,
            file_hash: Some(256),
            info: PluginInfo {
                name: "dprint-plugin-typescript".to_string(),
                version: "0.1.0".to_string(),
                config_key: "typescript".to_string(),
                file_extensions: vec![".ts".to_string()],
                help_url: "help url".to_string(),
                config_schema_url: "schema url".to_string()
            }
        });
        manifest.add_item(String::from("b"), PluginCacheManifestItem {
            created_time: 456,
            file_hash: None,
            info: PluginInfo {
                name: "dprint-plugin-json".to_string(),
                version: "0.2.0".to_string(),
                config_key: "json".to_string(),
                file_extensions: vec![".json".to_string()],
                help_url: "help url 2".to_string(),
                config_schema_url: "schema url 2".to_string()
            }
        });
        write_manifest(&manifest, &environment).unwrap();

        // Just read and compare again because the hash map will serialize properties
        // in a non-deterministic order.
        assert_eq!(read_manifest(&environment).unwrap(), manifest);
    }
}
