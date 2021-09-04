use parking_lot::RwLock;
use std::path::PathBuf;

use dprint_cli_core::checksums::verify_sha256_checksum;
use dprint_core::plugins::PluginInfo;
use dprint_core::types::ErrBox;

use super::implementations::{cleanup_plugin, get_file_path_from_plugin_info, setup_plugin};
use super::{read_manifest, write_manifest, PluginCacheManifest, PluginCacheManifestItem};
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::utils::{get_bytes_hash, PathSource};

pub struct PluginCacheItem {
  pub file_path: PathBuf,
  pub info: PluginInfo,
}

pub struct PluginCache<TEnvironment: Environment> {
  environment: TEnvironment,
  manifest: RwLock<PluginCacheManifest>,
}

impl<TEnvironment> PluginCache<TEnvironment>
where
  TEnvironment: Environment,
{
  pub fn new(environment: TEnvironment) -> Self {
    let manifest = RwLock::new(read_manifest(&environment));
    PluginCache { environment, manifest }
  }

  pub fn forget(&self, source_reference: &PluginSourceReference) -> Result<(), ErrBox> {
    let cache_key = self.get_cache_key(&source_reference.path_source)?;
    let mut manifest = self.manifest.write();
    let cache_item = manifest.remove_item(&cache_key);
    write_manifest(&manifest, &self.environment)?;

    if let Some(cache_item) = cache_item {
      match cleanup_plugin(&source_reference.path_source, &cache_item.info, &self.environment) {
        Err(err) => self.environment.log_stderr(&format!("Error forgetting plugin: {}", err.to_string())),
        _ => {}
      }
    }

    Ok(())
  }

  pub fn get_plugin_cache_item(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem, ErrBox> {
    match &source_reference.path_source {
      PathSource::Remote(_) => self.get_plugin(source_reference.clone(), false, download_url),
      PathSource::Local(_) => self.get_plugin(source_reference.clone(), true, get_file_bytes),
    }
  }

  fn get_plugin(
    &self,
    source_reference: PluginSourceReference,
    check_file_hash: bool,
    read_bytes: impl Fn(PathSource, TEnvironment) -> Result<Vec<u8>, ErrBox>,
  ) -> Result<PluginCacheItem, ErrBox> {
    let cache_key = self.get_cache_key(&source_reference.path_source)?;
    let cache_item = self.manifest.read().get_item(&cache_key).map(|x| x.to_owned()); // drop lock
    if let Some(cache_item) = cache_item {
      let file_path = get_file_path_from_plugin_info(&source_reference.path_source, &cache_item.info, &self.environment)?;

      if check_file_hash {
        let file_bytes = read_bytes(source_reference.path_source.clone(), self.environment.clone())?;
        let file_hash = get_bytes_hash(&file_bytes);
        let cache_file_hash = match &cache_item.file_hash {
          Some(file_hash) => *file_hash,
          None => return err!("Expected to have the plugin file hash stored in the cache."),
        };

        if file_hash == cache_file_hash {
          return Ok(PluginCacheItem {
            file_path,
            info: cache_item.info,
          });
        } else {
          self.forget(&source_reference)?;
        }
      } else {
        return Ok(PluginCacheItem {
          file_path,
          info: cache_item.info,
        });
      }
    }

    // get bytes
    let file_bytes = read_bytes(source_reference.path_source.clone(), self.environment.clone())?;

    // check checksum only if provided (not required for Wasm plugins)
    if let Some(checksum) = &source_reference.checksum {
      verify_sha256_checksum(&file_bytes, checksum)?;
    }

    let setup_result = setup_plugin(&source_reference.path_source, &file_bytes, &self.environment)?;
    let cache_item = PluginCacheManifestItem {
      info: setup_result.plugin_info.clone(),
      file_hash: if check_file_hash { Some(get_bytes_hash(&file_bytes)) } else { None },
      created_time: self.environment.get_time_secs(),
    };

    let mut manifest = self.manifest.write();
    manifest.add_item(cache_key, cache_item);
    write_manifest(&manifest, &self.environment)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
    })
  }

  fn get_cache_key(&self, path_source: &PathSource) -> Result<String, ErrBox> {
    Ok(match path_source {
      PathSource::Remote(remote_source) => format!("remote:{}", remote_source.url.as_str()),
      PathSource::Local(local_source) => {
        let absolute_path = self.environment.canonicalize(&local_source.path)?;
        format!("local:{}", absolute_path.to_string_lossy())
      }
    })
  }
}

fn download_url<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> Result<Vec<u8>, ErrBox> {
  environment.download_file(path_source.unwrap_remote().url.as_str())
}

fn get_file_bytes<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> Result<Vec<u8>, ErrBox> {
  environment.read_file_bytes(&path_source.unwrap_local().path)
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::plugins::{CompilationResult, PluginSourceReference};
  use dprint_core::plugins::PluginInfo;
  use dprint_core::types::ErrBox;
  use pretty_assertions::assert_eq;
  use std::path::PathBuf;

  #[test]
  fn it_should_download_remote_file() -> Result<(), ErrBox> {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", "t".as_bytes());
    environment.set_wasm_compile_result(create_compilation_result("t".as_bytes()));

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source)?.file_path;
    let expected_file_path = PathBuf::from("/cache").join("plugins").join("test-plugin").join("test-plugin-0.1.0.cached");

    assert_eq!(file_path, expected_file_path);
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source)?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":3,"plugins":{"remote:https://plugins.dprint.dev/test.wasm":{"createdTime":123456,"info":{"name":"test-plugin","version":"0.1.0","configKey":"test-plugin","fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url"}}}}"#,
    );

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":3,"plugins":{}}"#,
    );

    Ok(())
  }

  #[test]
  fn it_should_cache_local_file() -> Result<(), ErrBox> {
    let environment = TestEnvironment::new();
    let original_file_path = PathBuf::from("/test.wasm");
    let file_bytes = "t".as_bytes();
    environment.write_file_bytes(&original_file_path, file_bytes).unwrap();
    environment.set_wasm_compile_result(create_compilation_result("t".as_bytes()));

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_local(original_file_path.clone());
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source)?.file_path;
    let expected_file_path = PathBuf::from("/cache").join("plugins").join("test-plugin").join("test-plugin-0.1.0.cached");

    assert_eq!(file_path, expected_file_path);

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source)?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      concat!(
        r#"{"schemaVersion":3,"plugins":{"local:/test.wasm":{"createdTime":123456,"fileHash":10632242795325663332,"info":{"#,
        r#""name":"test-plugin","version":"0.1.0","configKey":"test-plugin","#,
        r#""fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url"}}}}"#,
      )
    );

    assert_eq!(environment.take_stderr_messages().len(), 0); // no logs, nothing changed

    // update the file bytes
    let file_bytes = "u".as_bytes();
    environment.write_file_bytes(&original_file_path, file_bytes).unwrap();

    // should update the cache with the new file
    let file_path = plugin_cache
      .get_plugin_cache_item(&PluginSourceReference::new_local(original_file_path.clone()))?
      .file_path;
    assert_eq!(file_path, expected_file_path);

    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      concat!(
        r#"{"schemaVersion":3,"plugins":{"local:/test.wasm":{"createdTime":123456,"fileHash":6989588595861227504,"info":{"#,
        r#""name":"test-plugin","version":"0.1.0","configKey":"test-plugin","#,
        r#""fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url"}}}}"#,
      )
    );

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":3,"plugins":{}}"#,
    );

    Ok(())
  }

  fn create_compilation_result(bytes: &[u8]) -> CompilationResult {
    CompilationResult {
      bytes: bytes.to_vec(),
      plugin_info: get_test_plugin_info(),
    }
  }

  fn get_test_plugin_info() -> PluginInfo {
    PluginInfo {
      name: String::from("test-plugin"),
      version: String::from("0.1.0"),
      config_key: String::from("test-plugin"),
      file_extensions: vec![String::from("txt"), String::from("dat")],
      file_names: vec![],
      help_url: String::from("test-url"),
      config_schema_url: String::from("schema-url"),
    }
  }
}
