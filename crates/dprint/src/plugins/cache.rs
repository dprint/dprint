use anyhow::bail;
use anyhow::Result;
use parking_lot::RwLock;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;

use super::implementations::cleanup_plugin;
use super::implementations::get_file_path_from_plugin_info;
use super::implementations::setup_plugin;
use super::read_manifest;
use super::write_manifest;
use super::PluginCacheManifest;
use super::PluginCacheManifestItem;
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::utils::get_bytes_hash;
use crate::utils::get_sha256_checksum;
use crate::utils::verify_sha256_checksum;
use crate::utils::PathSource;
use crate::utils::PluginKind;

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

  pub fn forget(&self, source_reference: &PluginSourceReference) -> Result<()> {
    let cache_key = self.get_cache_key(&source_reference.path_source)?;
    let mut manifest = self.manifest.write();
    let cache_item = manifest.remove_item(&cache_key);
    write_manifest(&manifest, &self.environment)?;

    if let Some(cache_item) = cache_item {
      if let Err(err) = cleanup_plugin(&source_reference.path_source, &cache_item.info, &self.environment) {
        self.environment.log_stderr(&format!("Error forgetting plugin: {:#}", err))
      }
    }

    Ok(())
  }

  pub async fn get_plugin_cache_item(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    match &source_reference.path_source {
      PathSource::Remote(_) => self.get_plugin(source_reference.clone(), false, download_url).await,
      PathSource::Local(_) => self.get_plugin(source_reference.clone(), true, get_file_bytes).await,
    }
  }

  async fn get_plugin(
    &self,
    source_reference: PluginSourceReference,
    check_file_hash: bool,
    read_bytes: impl Fn(PathSource, TEnvironment) -> Result<Vec<u8>>,
  ) -> Result<PluginCacheItem> {
    let cache_key = self.get_cache_key(&source_reference.path_source)?;
    let cache_item = self.manifest.read().get_item(&cache_key).map(|x| x.to_owned()); // drop lock
    if let Some(cache_item) = cache_item {
      let file_path = get_file_path_from_plugin_info(&source_reference.path_source, &cache_item.info, &self.environment)?;

      if check_file_hash {
        let file_bytes = read_bytes(source_reference.path_source.clone(), self.environment.clone())?;
        let file_hash = get_bytes_hash(&file_bytes);
        let cache_file_hash = match &cache_item.file_hash {
          Some(file_hash) => *file_hash,
          None => bail!("Expected to have the plugin file hash stored in the cache."),
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
      if let Err(err) = verify_sha256_checksum(&file_bytes, checksum) {
        bail!(
          "Invalid checksum specified in configuration file. Check the plugin's release notes for what the expected checksum is.\n\n{:#}",
          err
        );
      }
    } else if source_reference.plugin_kind() != Some(PluginKind::Wasm) {
      bail!(
        concat!(
          "The plugin must have a checksum specified for security reasons ",
          "since it is not a Wasm plugin. Check the plugin's release notes for what ",
          "the checksum is or if you trust the source, you may specify \"{}@{}\"."
        ),
        source_reference.path_source.display(),
        get_sha256_checksum(&file_bytes),
      );
    }

    let setup_result = setup_plugin(&source_reference.path_source, &file_bytes, &self.environment).await?;
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

  fn get_cache_key(&self, path_source: &PathSource) -> Result<String> {
    Ok(match path_source {
      PathSource::Remote(remote_source) => format!("remote:{}", remote_source.url.as_str()),
      PathSource::Local(local_source) => {
        let absolute_path = self.environment.canonicalize(&local_source.path)?;
        format!("local:{}", absolute_path.to_string_lossy())
      }
    })
  }
}

fn download_url<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> Result<Vec<u8>> {
  environment.download_file_err_404(path_source.unwrap_remote().url.as_str())
}

fn get_file_bytes<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> Result<Vec<u8>> {
  environment.read_file_bytes(path_source.unwrap_local().path)
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::plugins::CompilationResult;
  use crate::plugins::PluginSourceReference;
  use anyhow::Result;
  use dprint_core::plugins::PluginInfo;
  use pretty_assertions::assert_eq;
  use std::path::PathBuf;

  #[tokio::test]
  async fn should_download_remote_file() -> Result<()> {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", "t".as_bytes());
    environment.set_wasm_compile_result(create_compilation_result("t".as_bytes()));
    environment.set_cpu_arch("aarch64");

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    let expected_file_path = PathBuf::from("/cache").join("plugins").join("test-plugin").join("0.1.0-2.3.0-aarch64");

    assert_eq!(file_path, expected_file_path);
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":7,"wasmCacheVersion":"2.3.0","plugins":{"remote:https://plugins.dprint.dev/test.wasm":{"createdTime":123456,"info":{"name":"test-plugin","version":"0.1.0","configKey":"test-plugin","fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url","updateUrl":"update-url"}}}}"#,
    );

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":7,"wasmCacheVersion":"2.3.0","plugins":{}}"#,
    );

    Ok(())
  }

  #[tokio::test]
  async fn should_cache_local_file() -> Result<()> {
    let environment = TestEnvironment::new();
    let original_file_path = PathBuf::from("/test.wasm");
    let file_bytes = "t".as_bytes();
    environment.write_file_bytes(&original_file_path, file_bytes).unwrap();
    environment.set_wasm_compile_result(create_compilation_result("t".as_bytes()));

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_local(original_file_path.clone());
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    let expected_file_path = PathBuf::from("/cache").join("plugins").join("test-plugin").join("0.1.0-2.3.0-x86_64");

    assert_eq!(file_path, expected_file_path);

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      concat!(
        r#"{"schemaVersion":7,"wasmCacheVersion":"2.3.0","plugins":{"local:/test.wasm":{"createdTime":123456,"fileHash":10632242795325663332,"info":{"#,
        r#""name":"test-plugin","version":"0.1.0","configKey":"test-plugin","#,
        r#""fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url","updateUrl":"update-url"}}}}"#,
      )
    );

    assert_eq!(environment.take_stderr_messages().len(), 0); // no logs, nothing changed

    // update the file bytes
    let file_bytes = "u".as_bytes();
    environment.write_file_bytes(&original_file_path, file_bytes).unwrap();

    // should update the cache with the new file
    let file_path = plugin_cache
      .get_plugin_cache_item(&PluginSourceReference::new_local(original_file_path.clone()))
      .await?
      .file_path;
    assert_eq!(file_path, expected_file_path);

    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      concat!(
        r#"{"schemaVersion":7,"wasmCacheVersion":"2.3.0","plugins":{"local:/test.wasm":{"createdTime":123456,"fileHash":6989588595861227504,"info":{"#,
        r#""name":"test-plugin","version":"0.1.0","configKey":"test-plugin","#,
        r#""fileExtensions":["txt","dat"],"fileNames":[],"helpUrl":"test-url","configSchemaUrl":"schema-url","updateUrl":"update-url"}}}}"#,
      )
    );

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      r#"{"schemaVersion":7,"wasmCacheVersion":"2.3.0","plugins":{}}"#,
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
      update_url: Some(String::from("update-url")),
    }
  }
}
