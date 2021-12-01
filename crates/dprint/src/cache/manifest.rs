use dprint_core::types::ErrBox;
use serde::Deserialize;
use serde::Serialize;
use std::collections::hash_map::Values;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::environment::Environment;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct CacheManifest(HashMap<String, CacheItem>);

impl CacheManifest {
  pub(super) fn new() -> CacheManifest {
    CacheManifest(HashMap::new())
  }

  pub fn add_item(&mut self, key: String, item: CacheItem) {
    self.0.insert(key, item);
  }

  pub fn get_item(&self, key: &str) -> Option<&CacheItem> {
    self.0.get(key)
  }

  pub fn remove_item(&mut self, key: &str) -> Option<CacheItem> {
    self.0.remove(key)
  }

  pub fn items(&self) -> Values<'_, String, CacheItem> {
    self.0.values()
  }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CacheItem {
  pub(super) file_name: String,
  /// Created time in *seconds* since epoch.
  pub created_time: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub meta_data: Option<String>,
}

pub fn read_manifest(environment: &impl Environment) -> CacheManifest {
  let file_path = get_manifest_file_path(environment);
  match environment.read_file(&file_path) {
    Ok(text) => match serde_json::from_str(&text) {
      Ok(manifest) => manifest,
      Err(err) => {
        environment.log_stderr(&format!("Resetting cache manifest. Message: {}", err));
        CacheManifest::new()
      }
    },
    Err(_) => CacheManifest::new(),
  }
}

pub fn write_manifest(manifest: &CacheManifest, environment: &impl Environment) -> Result<(), ErrBox> {
  let file_path = get_manifest_file_path(environment);
  let serialized_manifest = serde_json::to_string(&manifest)?;
  environment.write_file(&file_path, &serialized_manifest)
}

fn get_manifest_file_path(environment: &impl Environment) -> PathBuf {
  let cache_dir = environment.get_cache_dir();
  cache_dir.join("cache-manifest.json")
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
        &environment.get_cache_dir().join("cache-manifest.json"),
        r#"{
    "a": {
        "fileName": "b",
        "createdTime": 123
    },
    "c": {
        "fileName": "d",
        "createdTime": 456,
        "metaData": "{\"test\":5}"
    }
}"#,
      )
      .unwrap();

    let mut expected_manifest = CacheManifest::new();
    expected_manifest.add_item(
      String::from("a"),
      CacheItem {
        file_name: String::from("b"),
        created_time: 123,
        meta_data: None,
      },
    );
    expected_manifest.add_item(
      String::from("c"),
      CacheItem {
        file_name: String::from("d"),
        created_time: 456,
        meta_data: Some(String::from("{\"test\":5}")),
      },
    );

    assert_eq!(read_manifest(&environment), expected_manifest);
  }

  #[test]
  fn should_have_empty_manifest_for_deserialization_error() {
    let environment = TestEnvironment::new();
    environment
      .write_file(
        &environment.get_cache_dir().join("cache-manifest.json"),
        r#"{ "a": { file_name: "b", "createdTime": 123 } }"#,
      )
      .unwrap();

    assert_eq!(read_manifest(&environment), CacheManifest::new());
    assert_eq!(
      environment.take_stderr_messages(),
      vec![String::from("Resetting cache manifest. Message: key must be a string at line 1 column 10")]
    );
  }

  #[test]
  fn should_deal_with_non_existent_manifest() {
    let environment = TestEnvironment::new();

    assert_eq!(read_manifest(&environment), CacheManifest::new());
    assert_eq!(environment.take_stderr_messages().len(), 0);
  }

  #[test]
  fn it_save_manifest() {
    let environment = TestEnvironment::new();
    let mut manifest = CacheManifest::new();
    manifest.add_item(
      String::from("a"),
      CacheItem {
        file_name: String::from("b"),
        created_time: 123,
        meta_data: None,
      },
    );
    manifest.add_item(
      String::from("c"),
      CacheItem {
        file_name: String::from("d"),
        created_time: 456,
        meta_data: Some(String::from("test")),
      },
    );
    write_manifest(&manifest, &environment).unwrap();

    // Just read and compare again because the hash map will serialize properties
    // in a non-deterministic order.
    assert_eq!(read_manifest(&environment), manifest);
  }
}
