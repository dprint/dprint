use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use dprint_core::plugins::PluginInfo;

use crate::environment::Environment;
use crate::types::ErrBox;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct CacheManifest {
    pub urls: Vec<UrlCacheEntry>,
}

impl CacheManifest {
    pub(super) fn new() -> CacheManifest {
        CacheManifest { urls: vec![] }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UrlCacheEntry {
    pub url: String,
    pub file_name: String,
    /// Created time in *seconds* since epoch.
    pub created_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_info: Option<PluginInfo>,
}

pub fn read_manifest(environment: &impl Environment) -> Result<CacheManifest, ErrBox> {
    let file_path = get_manifest_file_path(environment)?;
    let manifest_file_text = match environment.read_file(&file_path) {
        Ok(text) => Some(text),
        Err(_) => None,
    };

    if let Some(text) = manifest_file_text {
        let deserialized_manifest = serde_json::from_str(&text);
        match deserialized_manifest {
            Ok(manifest) => Ok(manifest),
            Err(err) => {
                environment.log_error(&format!("Error deserializing cache manifest, but ignoring: {}", err));
                Ok(CacheManifest::new())
            }
        }
    } else {
        Ok(CacheManifest::new())
    }
}

pub fn write_manifest(manifest: &CacheManifest, environment: &impl Environment) -> Result<(), ErrBox> {
    let file_path = get_manifest_file_path(environment)?;
    let serialized_manifest = serde_json::to_string(&manifest).unwrap();
    environment.write_file(&file_path, &serialized_manifest)
}

fn get_manifest_file_path(environment: &impl Environment) -> Result<PathBuf, ErrBox> {
    let cache_dir = environment.get_cache_dir()?;
    Ok(cache_dir.join("cache-manifest.json"))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::environment::TestEnvironment;

    #[test]
    fn it_should_read_ok_manifest() {
        let environment = TestEnvironment::new();
        environment.write_file(
            &environment.get_cache_dir().unwrap().join("cache-manifest.json"),
            r#"{
    "urls": [{
        "url": "a",
        "fileName": "b",
        "createdTime": 123
    },{
        "url": "c",
        "fileName": "d",
        "createdTime": 456,
        "pluginInfo": {
            "name": "test-plugin",
            "version": "0.1.0",
            "configKeys": ["test-plugin"],
            "fileExtensions": ["txt","dat"],
            "helpUrl": "test-url",
            "configSchemaUrl": "schema-url"
        }
    }]
}"#
        ).unwrap();

        assert_eq!(read_manifest(&environment).unwrap(), CacheManifest {
            urls: vec![UrlCacheEntry {
                url: String::from("a"),
                file_name: String::from("b"),
                created_time: 123,
                plugin_info: None,
            }, UrlCacheEntry {
                url: String::from("c"),
                file_name: String::from("d"),
                created_time: 456,
                plugin_info: Some(get_test_plugin_info()),
            }]
        })
    }

    #[test]
    fn it_should_have_empty_manifest_for_deserialization_error() {
        let environment = TestEnvironment::new();
        environment.write_file(
            &environment.get_cache_dir().unwrap().join("cache-manifest.json"),
            r#"{ "urls": [{ "url": "a", file_name: "b", "createdTime": 123 }] }"#
        ).unwrap();

        assert_eq!(read_manifest(&environment).unwrap(), CacheManifest::new());
        assert_eq!(environment.get_logged_errors(), vec![
            String::from("Error deserializing cache manifest, but ignoring: key must be a string at line 1 column 26")
        ]);
    }

    #[test]
    fn it_should_deal_with_non_existent_manifest() {
        let environment = TestEnvironment::new();

        assert_eq!(read_manifest(&environment).unwrap(), CacheManifest::new());
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_save_manifest() {
        let environment = TestEnvironment::new();
        let manifest = CacheManifest {
            urls: vec![
                UrlCacheEntry {
                    url: String::from("a"),
                    file_name: String::from("b"),
                    created_time: 123,
                    plugin_info: None,
                },
                UrlCacheEntry {
                    url: String::from("c"),
                    file_name: String::from("d"),
                    created_time: 456,
                    plugin_info: Some(get_test_plugin_info()),
                },
            ]
        };
        write_manifest(&manifest, &environment).unwrap();
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{"urls":[{"url":"a","fileName":"b","createdTime":123},{"url":"c","fileName":"d","createdTime":456,"pluginInfo":{"name":"test-plugin","version":"0.1.0","configKeys":["test-plugin"],"fileExtensions":["txt","dat"],"helpUrl":"test-url","configSchemaUrl":"schema-url"}}]}"#
        );
    }

    fn get_test_plugin_info() -> PluginInfo {
        PluginInfo {
            name: String::from("test-plugin"),
            version: String::from("0.1.0"),
            config_keys: vec![String::from("test-plugin")],
            file_extensions: vec![String::from("txt"), String::from("dat")],
            help_url: String::from("test-url"),
            config_schema_url: String::from("schema-url"),
        }
    }
}
