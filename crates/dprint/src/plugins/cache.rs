use anyhow::Result;
use anyhow::bail;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;
use parking_lot::RwLock;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;

use super::PluginCacheManifest;
use super::PluginCacheManifestItem;
use super::cache_fs_locks::CacheFsLockPool;
use super::implementations::cleanup_plugin;
use super::implementations::get_file_path_from_plugin_info;
use super::implementations::setup_plugin;
use super::read_manifest;
use super::write_manifest;
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::get_bytes_hash;
use crate::utils::get_sha256_checksum;
use crate::utils::verify_sha256_checksum;

pub struct PluginCacheItem {
  pub file_path: PathBuf,
  pub info: PluginInfo,
}

pub struct PluginCache<TEnvironment: Environment> {
  environment: TEnvironment,
  manifest: ConcurrentPluginCacheManifest<TEnvironment>,
  fs_locks: CacheFsLockPool<TEnvironment>,
}

impl<TEnvironment> PluginCache<TEnvironment>
where
  TEnvironment: Environment,
{
  pub fn new(environment: TEnvironment) -> Self {
    PluginCache {
      manifest: ConcurrentPluginCacheManifest::new(environment.clone()),
      fs_locks: CacheFsLockPool::new(environment.clone()),
      environment,
    }
  }

  pub async fn forget_and_recreate(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    let _setup_guard = self.fs_locks.lock(&source_reference.path_source).await;
    self.forget(source_reference).await?;
    self.get_plugin_cache_item(source_reference).await
  }

  pub async fn forget(&self, source_reference: &PluginSourceReference) -> Result<()> {
    let _setup_guard = self.fs_locks.lock(&source_reference.path_source).await;
    let removed_cache_item = self.manifest.remove(&source_reference.path_source)?;

    if let Some(cache_item) = removed_cache_item
      && let Err(err) = cleanup_plugin(&source_reference.path_source, &cache_item.info, &self.environment)
    {
      log_warn!(self.environment, "Error forgetting plugin: {:#}", err);
    }

    Ok(())
  }

  pub async fn get_plugin_cache_item(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    match &source_reference.path_source {
      PathSource::Remote(_) => self.get_plugin(source_reference, false, download_url).await,
      PathSource::Local(_) => {
        if let Some(manifest_item) = self.manifest.get(&source_reference.path_source)? {
          let file_bytes = get_file_bytes(source_reference.path_source.clone(), self.environment.clone()).await?;
          let file_hash = get_bytes_hash(&file_bytes);
          let cache_file_hash = match &manifest_item.file_hash {
            Some(file_hash) => *file_hash,
            None => bail!("Expected to have the plugin file hash stored in the cache."),
          };

          if file_hash == cache_file_hash {
            return Ok(PluginCacheItem {
              file_path: get_file_path_from_plugin_info(&source_reference.path_source, &manifest_item.info, &self.environment)?,
              info: manifest_item.info,
            });
          } else {
            self.forget(source_reference).await?;
          }
        }

        self.get_plugin(source_reference, true, get_file_bytes).await
      }
    }
  }

  async fn get_plugin(
    &self,
    source_reference: &PluginSourceReference,
    include_file_hash: bool,
    read_bytes: impl Fn(PathSource, TEnvironment) -> LocalBoxFuture<'static, Result<Vec<u8>>>,
  ) -> Result<PluginCacheItem> {
    if let Some(item) = self.get_plugin_cache_item_from_cache(&source_reference.path_source)? {
      return Ok(item);
    }

    // prevent multiple processes from downloading the same plugin at the same time
    let _setup_guard = self.fs_locks.lock(&source_reference.path_source).await;

    // once in the lock, attempt to reload and see if the item is in the cache now
    self.manifest.reload_from_disk();
    if let Some(item) = self.get_plugin_cache_item_from_cache(&source_reference.path_source)? {
      return Ok(item);
    }

    // get bytes
    let file_bytes = read_bytes(source_reference.path_source.clone(), self.environment.clone()).await?;

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
          "the checksum is or if you trust the source, you may specify: {}@{}"
        ),
        source_reference.path_source.display(),
        get_sha256_checksum(&file_bytes),
      );
    }

    let file_hash = if include_file_hash { Some(get_bytes_hash(&file_bytes)) } else { None };
    let setup_result = setup_plugin(&source_reference.path_source, file_bytes, &self.environment).await?;
    let cache_item = PluginCacheManifestItem {
      info: setup_result.plugin_info.clone(),
      file_hash,
      created_time: self.environment.get_time_secs(),
    };

    self.manifest.add(&source_reference.path_source, cache_item)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
    })
  }

  fn get_plugin_cache_item_from_cache(&self, path_source: &PathSource) -> Result<Option<PluginCacheItem>> {
    if let Some(item) = self.manifest.get(path_source)? {
      Ok(Some(PluginCacheItem {
        file_path: get_file_path_from_plugin_info(path_source, &item.info, &self.environment)?,
        info: item.info,
      }))
    } else {
      Ok(None)
    }
  }
}

struct ConcurrentPluginCacheManifest<TEnvironment: Environment> {
  environment: TEnvironment,
  manifest: RwLock<PluginCacheManifest>,
}

impl<TEnvironment: Environment> ConcurrentPluginCacheManifest<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    let manifest = RwLock::new(read_manifest(&environment));
    Self { environment, manifest }
  }

  pub fn get(&self, path_source: &PathSource) -> Result<Option<PluginCacheManifestItem>> {
    let cache_key = self.get_cache_key(path_source)?;
    Ok(self.manifest.read().get_item(&cache_key).map(|x| x.to_owned()))
  }

  pub fn add(&self, path_source: &PathSource, cache_item: PluginCacheManifestItem) -> Result<()> {
    let mut manifest = self.manifest.write();
    manifest.add_item(self.get_cache_key(path_source)?, cache_item);
    write_manifest(&manifest, &self.environment)?;
    Ok(())
  }

  pub fn remove(&self, path_source: &PathSource) -> Result<Option<PluginCacheManifestItem>> {
    let cache_key = self.get_cache_key(path_source)?;
    let mut manifest = self.manifest.write();
    let cache_item = manifest.remove_item(&cache_key);
    write_manifest(&manifest, &self.environment)?;
    Ok(cache_item)
  }

  pub fn reload_from_disk(&self) {
    // ensure the lock is held while reading from the file system
    // in order to prevent another thread writing to the file system
    // at the same time
    let mut manifest = self.manifest.write();
    *manifest = read_manifest(&self.environment);
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

fn download_url<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> LocalBoxFuture<'static, Result<Vec<u8>>> {
  async move { environment.download_file_err_404(path_source.unwrap_remote().url.as_str()).await }.boxed_local()
}

fn get_file_bytes<TEnvironment: Environment>(path_source: PathSource, environment: TEnvironment) -> LocalBoxFuture<'static, Result<Vec<u8>>> {
  async move { environment.read_file_bytes(path_source.unwrap_local().path) }.boxed_local()
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::plugins::PluginSourceReference;
  use crate::plugins::implementations::WASMER_COMPILER_VERSION;
  use crate::test_helpers::WASM_PLUGIN_0_1_0_BYTES;
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use anyhow::Result;
  use pretty_assertions::assert_eq;
  use std::path::PathBuf;

  #[tokio::test]
  async fn should_download_remote_file() -> Result<()> {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", WASM_PLUGIN_BYTES);
    environment.set_cpu_arch("aarch64");

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    let expected_file_path = PathBuf::from("/cache")
      .join("plugins")
      .join("test-plugin")
      .join(format!("0.2.0-{WASMER_COMPILER_VERSION}-aarch64"));

    assert_eq!(file_path, expected_file_path);
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      serde_json::json!({
        "schemaVersion": 8,
        "wasmCacheVersion": WASMER_COMPILER_VERSION,
        "plugins": {
          "remote:https://plugins.dprint.dev/test.wasm": {
            "createdTime": 123456,
            "info": {
              "name": "test-plugin",
              "version": "0.2.0",
              "configKey": "test-plugin",
              "helpUrl": "https://dprint.dev/plugins/test",
              "configSchemaUrl": "https://plugins.dprint.dev/test/schema.json",
              "updateUrl": "https://plugins.dprint.dev/dprint/test-plugin/latest.json"
            }
          }
        }
      })
      .to_string(),
    );

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).await.unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      serde_json::json!({
        "schemaVersion": 8,
        "wasmCacheVersion": WASMER_COMPILER_VERSION,
        "plugins": {}
      })
      .to_string(),
    );

    Ok(())
  }

  #[tokio::test]
  async fn should_cache_local_file() -> Result<()> {
    let environment = TestEnvironment::new();
    let original_file_path = PathBuf::from("/test.wasm");
    environment.write_file_bytes(&original_file_path, &WASM_PLUGIN_BYTES).unwrap();

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_local(original_file_path.clone());
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    let expected_file_path = PathBuf::from("/cache")
      .join("plugins")
      .join("test-plugin")
      .join(format!("0.2.0-{WASMER_COMPILER_VERSION}-x86_64"));

    assert_eq!(file_path, expected_file_path);

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should be the same when requesting it again
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);

    // should have saved the manifest
    let expected_text = serde_json::json!({
      "schemaVersion": 8,
      "wasmCacheVersion": WASMER_COMPILER_VERSION,
      "plugins": {
        "local:/test.wasm": {
          "createdTime": 123456,
          "fileHash": get_bytes_hash(&WASM_PLUGIN_BYTES),
          "info": {
            "name": "test-plugin",
            "version": "0.2.0",
            "configKey": "test-plugin",
            "helpUrl": "https://dprint.dev/plugins/test",
            "configSchemaUrl": "https://plugins.dprint.dev/test/schema.json",
            "updateUrl": "https://plugins.dprint.dev/dprint/test-plugin/latest.json"
          }
        }
      }
    });
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      expected_text.to_string(),
    );

    assert_eq!(environment.take_stderr_messages().len(), 0); // no logs, nothing changed

    // update the file bytes
    environment.write_file_bytes(&original_file_path, &WASM_PLUGIN_0_1_0_BYTES).unwrap();

    // should update the cache with the new file
    let expected_file_path = PathBuf::from("/cache")
      .join("plugins")
      .join("test-plugin")
      .join(format!("0.1.0-{WASMER_COMPILER_VERSION}-x86_64"));
    let file_path = plugin_cache
      .get_plugin_cache_item(&PluginSourceReference::new_local(original_file_path.clone()))
      .await?
      .file_path;
    assert_eq!(file_path, expected_file_path);

    let expected_text = serde_json::json!({
      "schemaVersion": 8,
      "wasmCacheVersion": WASMER_COMPILER_VERSION,
      "plugins": {
        "local:/test.wasm": {
          "createdTime": 123456,
          "fileHash": get_bytes_hash(&WASM_PLUGIN_0_1_0_BYTES),
          "info": {
            "name": "test-plugin",
            "version": "0.1.0",
            "configKey": "test-plugin",
            "helpUrl": "https://dprint.dev/plugins/test",
            "configSchemaUrl": "https://plugins.dprint.dev/test/schema.json",
            "updateUrl": "https://plugins.dprint.dev/dprint/test-plugin/latest.json"
          }
        }
      }
    });
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      expected_text.to_string()
    );

    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // should forget it afterwards
    plugin_cache.forget(&plugin_source).await.unwrap();

    assert_eq!(environment.path_exists(&file_path), false);
    // should have saved the manifest
    assert_eq!(
      environment.read_file(&environment.get_cache_dir().join("plugin-cache-manifest.json")).unwrap(),
      serde_json::json!({
        "schemaVersion": 8,
        "wasmCacheVersion": WASMER_COMPILER_VERSION,
        "plugins": {}
      })
      .to_string(),
    );

    Ok(())
  }
}
