use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use dprint_core::plugins::PluginInfo;

use super::PluginCacheManifest;
use super::PluginCacheManifestItem;
use super::cache_fs_locks::CacheFsLockPool;
use super::implementations::cleanup_plugin;
use super::implementations::get_file_path_from_plugin_info;
use super::implementations::setup_plugin;
use super::npm_resolution;
use super::read_manifest;
use super::write_manifest;
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::utils::FastInsecureHasher;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::get_bytes_hash;
use crate::utils::get_sha256_checksum;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;
use std::hash::Hasher;

pub struct PluginCacheItem {
  pub file_path: PathBuf,
  pub info: PluginInfo,
  pub plugin_kind: PluginKind,
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
    // lock on the same key the inner forget+resolve will use so this whole
    // forget-then-recreate sequence appears atomic to other processes. See
    // `cache_lock_key` for why npm needs special treatment here.
    let lock_source = self.cache_lock_key(&source_reference.path_source);
    let _setup_guard = self.fs_locks.lock(&lock_source).await;
    self.forget(source_reference).await?;
    self.get_plugin_cache_item(source_reference).await
  }

  pub async fn forget(&self, source_reference: &PluginSourceReference) -> Result<()> {
    // Unversioned npm specifiers cache under their resolved local node_modules
    // path, not under the npm path. Translate so manifest.remove targets the
    // same key the resolve flow stored under. If the package is no longer in
    // node_modules we have no path to key on, so there's nothing to forget.
    let Some(manifest_source) = self.manifest_source_for_forget(&source_reference.path_source) else {
      return Ok(());
    };
    // Lock on the same key the resolve flow uses (`get_local_plugin` for
    // unversioned npm), not the original npm PathSource — otherwise a
    // concurrent process resolving the same plugin would lock on a
    // different key and we'd race between cleanup_plugin here and
    // setup_plugin over there.
    let _setup_guard = self.fs_locks.lock(&manifest_source).await;
    let removed_cache_item = self.manifest.remove(&manifest_source)?;

    if let Some(cache_item) = removed_cache_item {
      let plugin_kind = cache_item.plugin_kind.or_else(|| source_reference.plugin_kind()).unwrap_or(PluginKind::Wasm);
      if let Err(err) = cleanup_plugin(plugin_kind, &cache_item.info, &self.environment) {
        log_warn!(self.environment, "Error forgetting plugin: {:#}", err);
      }
      // also remove the npm tarball extract dir for versioned npm specifiers
      if let PathSource::Npm(npm_source) = &source_reference.path_source
        && let Some(version) = &npm_source.specifier.version
      {
        let start_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
        let registry = self.manifest.resolve_registry_url(&npm_source.specifier.name, start_dir);
        let registry_segment = npm_resolution::registry_dir_segment(&registry);
        let extract_dir = npm_resolution::get_npm_extract_dir(&registry_segment, &npm_source.specifier.name, version, &self.environment);
        let _ = self.environment.remove_dir_all(&extract_dir);
      }
    }

    Ok(())
  }

  /// Returns the `PathSource` to lock on when forgetting *and* recreating
  /// a plugin atomically. For unversioned npm this is the resolved local
  /// node_modules path (same key the resolve flow's `get_local_plugin`
  /// locks on); for everything else it's the source itself. Falls back to
  /// the original source if the local path can't be resolved — the
  /// subsequent resolve will then fail with a clearer error anyway.
  fn cache_lock_key(&self, path_source: &PathSource) -> PathSource {
    self.manifest_source_for_forget(path_source).unwrap_or_else(|| path_source.clone())
  }

  /// Returns the `PathSource` whose cache key matches how the resolve flow
  /// stored the manifest entry. For unversioned npm this is the resolved local
  /// node_modules path; for everything else it's the source itself. Returns
  /// `None` for an unversioned npm whose package is no longer in node_modules
  /// — without a local path we have no key to look up, so there's nothing to
  /// forget.
  fn manifest_source_for_forget(&self, path_source: &PathSource) -> Option<PathSource> {
    if let PathSource::Npm(npm_source) = path_source
      && npm_source.specifier.version.is_none()
    {
      let base_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
      let fallback_dir = self.environment.cwd();
      let config_dir = base_dir.unwrap_or(fallback_dir.as_ref());
      return npm_resolution::find_npm_plugin_local_path(&npm_source.specifier, config_dir, &self.environment).ok();
    }
    Some(path_source.clone())
  }

  pub async fn get_plugin_cache_item(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    // for local plugins, check if the file changed since it was cached
    match &source_reference.path_source {
      PathSource::Remote(_) => {}
      PathSource::Local(local) => {
        if let Some(manifest_item) = self.manifest.get(&source_reference.path_source)? {
          let file_bytes = self.environment.read_file_bytes(&local.path)?;
          let plugin_kind = manifest_item
            .plugin_kind
            .or_else(|| source_reference.plugin_kind())
            .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;
          let file_hash = compute_local_plugin_file_hash(&source_reference.path_source, &file_bytes, plugin_kind, None, &self.environment);
          let cache_file_hash = match &manifest_item.file_hash {
            Some(file_hash) => *file_hash,
            None => bail!("Expected to have the plugin file hash stored in the cache."),
          };

          if file_hash == cache_file_hash {
            return Ok(PluginCacheItem {
              file_path: get_file_path_from_plugin_info(plugin_kind, &manifest_item.info, &self.environment),
              info: manifest_item.info,
              plugin_kind,
            });
          } else {
            self.forget(source_reference).await?;
          }
        }
      }
      PathSource::Npm(npm_source) => {
        return if npm_source.specifier.version.is_some() {
          self.get_npm_registry_plugin(source_reference, npm_source).await
        } else {
          // resolve to a local path, then delegate to the local plugin caching
          // so file_hash change detection works the same as any local plugin
          let base_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
          let fallback_dir = self.environment.cwd();
          let config_dir = base_dir.unwrap_or(fallback_dir.as_ref());
          let resolved = npm_resolution::resolve_npm_from_node_modules(&npm_source.specifier, config_dir, &self.environment)
            .await
            .with_context(|| format!("Resolving {}", npm_source.specifier.display()))?;
          let local_ref = PluginSourceReference {
            path_source: resolved.local_path,
            checksum: None,
          };
          self
            .get_local_plugin(&local_ref, resolved.pre_resolved_tarball)
            .await
            .with_context(|| format!("Setting up {}", npm_source.specifier.display()))
        };
      }
    }

    self.get_plugin(source_reference).await
  }

  async fn get_npm_registry_plugin(&self, source_reference: &PluginSourceReference, npm_source: &crate::utils::NpmPathSource) -> Result<PluginCacheItem> {
    // check cache first
    if let Some(item) = self.get_plugin_cache_item_from_cache(&source_reference.path_source)? {
      return Ok(item);
    }

    let _setup_guard = self.fs_locks.lock(&source_reference.path_source).await;

    // reload and re-check after acquiring lock
    self.manifest.reload_from_disk();
    if let Some(item) = self.get_plugin_cache_item_from_cache(&source_reference.path_source)? {
      return Ok(item);
    }

    let specifier = &npm_source.specifier;
    let checksum = source_reference.checksum.as_deref();
    let base_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
    let registry = self.manifest.resolve_registry(&specifier.name, base_dir);
    let resolved = npm_resolution::resolve_npm_from_registry(specifier, checksum, &registry, base_dir, &self.environment).await?;

    let plugin_kind = resolved.plugin_kind;
    // use the local extracted path so process plugin manifests can resolve relative URLs
    let setup_result = setup_plugin(
      &resolved.local_path,
      resolved.plugin_bytes,
      plugin_kind,
      resolved.pre_resolved_tarball,
      &self.environment,
    )
    .await
    .with_context(|| format!("Setting up {}", specifier.display()))?;
    let cache_item = PluginCacheManifestItem {
      info: setup_result.plugin_info.clone(),
      file_hash: None,
      plugin_kind: Some(plugin_kind),
      created_time: self.environment.get_time_secs(),
    };

    self.manifest.add(&source_reference.path_source, cache_item)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
      plugin_kind,
    })
  }

  /// Gets a plugin from a local path (node_modules). No checksum required since it's a local file.
  async fn get_local_plugin(
    &self,
    source_reference: &PluginSourceReference,
    pre_resolved_tarball: Option<npm_resolution::PreResolvedProcessPluginTarball>,
  ) -> Result<PluginCacheItem> {
    let local_path = source_reference
      .path_source
      .maybe_local_path()
      .ok_or_else(|| anyhow::anyhow!("Expected local path for npm node_modules plugin"))?;

    // for npm-resolved process plugins the per-platform binary comes from the
    // npm registry (verified against plugin.json's checksum), so the plugin
    // file's bytes alone fully determine the cached output. For process
    // plugins whose per-platform reference points to a *local* archive
    // (relative path / file://), [`compute_local_plugin_file_hash`] also
    // mixes the archive's bytes in so editing it invalidates the cache.

    // check file hash to see if we can reuse cached setup
    if let Some(manifest_item) = self.manifest.get(&source_reference.path_source)? {
      let file_bytes = self.environment.read_file_bytes(local_path)?;
      let plugin_kind = manifest_item
        .plugin_kind
        .or_else(|| source_reference.plugin_kind())
        .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;
      let file_hash = compute_local_plugin_file_hash(
        &source_reference.path_source,
        &file_bytes,
        plugin_kind,
        pre_resolved_tarball.as_ref(),
        &self.environment,
      );
      let cache_file_hash = match &manifest_item.file_hash {
        Some(file_hash) => *file_hash,
        None => bail!("Expected to have the plugin file hash stored in the cache."),
      };

      if file_hash == cache_file_hash {
        return Ok(PluginCacheItem {
          file_path: get_file_path_from_plugin_info(plugin_kind, &manifest_item.info, &self.environment),
          info: manifest_item.info,
          plugin_kind,
        });
      } else {
        self.forget(source_reference).await?;
      }
    }

    let _setup_guard = self.fs_locks.lock(&source_reference.path_source).await;
    self.manifest.reload_from_disk();
    if let Some(item) = self.get_plugin_cache_item_from_cache(&source_reference.path_source)? {
      return Ok(item);
    }

    let file_bytes = self.environment.read_file_bytes(local_path)?;
    let plugin_kind = source_reference
      .plugin_kind()
      .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;

    let file_hash = Some(compute_local_plugin_file_hash(
      &source_reference.path_source,
      &file_bytes,
      plugin_kind,
      pre_resolved_tarball.as_ref(),
      &self.environment,
    ));
    let setup_result = setup_plugin(&source_reference.path_source, file_bytes, plugin_kind, pre_resolved_tarball, &self.environment).await?;
    let cache_item = PluginCacheManifestItem {
      info: setup_result.plugin_info.clone(),
      file_hash,
      plugin_kind: Some(plugin_kind),
      created_time: self.environment.get_time_secs(),
    };

    self.manifest.add(&source_reference.path_source, cache_item)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
      plugin_kind,
    })
  }

  async fn get_plugin(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
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

    // get bytes (resolved_source may differ from the original due to redirects)
    let (file_bytes, resolved_source) = match &source_reference.path_source {
      PathSource::Remote(remote) => {
        let (url, file) = self.environment.download_file_err_404(&remote.url, None).await?;
        (file.content, PathSource::new_remote(url.into_owned()))
      }
      PathSource::Local(local) => {
        let bytes = self.environment.read_file_bytes(&local.path)?;
        (bytes, source_reference.path_source.clone())
      }
      PathSource::Npm(_) => bail!("npm plugins should be resolved before reaching get_plugin"),
    };

    let plugin_kind = source_reference
      .plugin_kind()
      .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;

    // check checksum only if provided (not required for Wasm plugins)
    if let Some(checksum) = &source_reference.checksum {
      if let Err(err) = verify_sha256_checksum(&file_bytes, checksum) {
        bail!(
          "Invalid checksum specified in configuration file. Check the plugin's release notes for what the expected checksum is.\n\n{:#}",
          err
        );
      }
    } else if plugin_kind != PluginKind::Wasm {
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

    let file_hash = match &resolved_source {
      PathSource::Local(_) => Some(compute_local_plugin_file_hash(
        &resolved_source,
        &file_bytes,
        plugin_kind,
        None,
        &self.environment,
      )),
      _ => None,
    };
    let setup_result = setup_plugin(&resolved_source, file_bytes, plugin_kind, None, &self.environment).await?;
    let cache_item = PluginCacheManifestItem {
      info: setup_result.plugin_info.clone(),
      file_hash,
      plugin_kind: Some(plugin_kind),
      created_time: self.environment.get_time_secs(),
    };

    self.manifest.add(&source_reference.path_source, cache_item)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
      plugin_kind,
    })
  }

  fn get_plugin_cache_item_from_cache(&self, path_source: &PathSource) -> Result<Option<PluginCacheItem>> {
    if let Some(item) = self.manifest.get(path_source)? {
      let plugin_kind = item
        .plugin_kind
        .or_else(|| path_source.plugin_kind())
        .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for cached plugin {}", path_source.display()))?;
      Ok(Some(PluginCacheItem {
        file_path: get_file_path_from_plugin_info(plugin_kind, &item.info, &self.environment),
        info: item.info,
        plugin_kind,
      }))
    } else {
      Ok(None)
    }
  }
}

/// Cache invalidation hash for a local plugin file. For wasm or for process
/// plugins whose per-platform archive comes from an `npm:` reference (we
/// already have the verified tarball in hand via `pre_resolved_tarball`),
/// the plugin file's bytes alone determine the cached output. For process
/// plugins whose per-platform reference resolves to a *local* archive
/// (relative path / `file://`), mix that archive's bytes into the hash —
/// otherwise editing `bin.zip` without touching `plugin.json` would leave
/// the previously-extracted executable in the cache forever and skip the
/// checksum check that `setup_plugin` would do on a fresh extract.
///
/// Failing-soft (returning just the primary hash) when the manifest can't
/// be parsed or the local archive can't be read is intentional — those
/// errors will surface with much better context from `setup_plugin` when
/// the cache miss falls through.
fn compute_local_plugin_file_hash<TEnvironment: Environment>(
  source: &PathSource,
  plugin_bytes: &[u8],
  plugin_kind: PluginKind,
  pre_resolved_tarball: Option<&npm_resolution::PreResolvedProcessPluginTarball>,
  environment: &TEnvironment,
) -> u64 {
  if plugin_kind != PluginKind::Process || pre_resolved_tarball.is_some() {
    return get_bytes_hash(plugin_bytes);
  }
  let Some(archive_bytes) = read_local_per_platform_archive_bytes(source, plugin_bytes, environment) else {
    return get_bytes_hash(plugin_bytes);
  };
  let mut hasher = FastInsecureHasher::default();
  hasher.write(plugin_bytes);
  hasher.write(&archive_bytes);
  hasher.finish()
}

/// If `plugin_bytes` is a parseable process-plugin manifest whose
/// per-platform reference resolves to a local file (relative path /
/// `file://`), returns that file's bytes. Returns `None` for any other
/// case — non-local references, parse failures, missing platform entry,
/// or unreadable archive. Errors are swallowed because callers fall back
/// to a primary-only hash and the real setup will surface the same errors
/// with better context.
fn read_local_per_platform_archive_bytes<TEnvironment: Environment>(source: &PathSource, plugin_bytes: &[u8], environment: &TEnvironment) -> Option<Vec<u8>> {
  use crate::plugins::implementations::get_process_plugin_os_path;
  use crate::plugins::implementations::parse_process_plugin_file;

  let plugin_file = parse_process_plugin_file(plugin_bytes).ok()?;
  let os_path = get_process_plugin_os_path(&plugin_file, environment).ok()?;
  // an `npm:` reference is handled via pre_resolved_tarball; an http(s) one
  // is fetched fresh during setup and rejected for npm-installed plugins.
  // we only need to mix in bytes for *local* references.
  if os_path.reference.starts_with("npm:") || os_path.reference.starts_with("http://") || os_path.reference.starts_with("https://") {
    return None;
  }
  let resolved = resolve_url_or_file_path_to_path_source(&os_path.reference, &source.parent(), environment).ok()?;
  let PathSource::Local(local) = resolved else {
    return None;
  };
  environment.read_file_bytes(&local.path).ok()
}

struct ConcurrentPluginCacheManifest<TEnvironment: Environment> {
  environment: TEnvironment,
  manifest: RwLock<PluginCacheManifest>,
  /// memoized npm registry resolutions keyed on (package name, config dir).
  /// resolving via `.npmrc` walks the directory tree, so the same key is
  /// hit multiple times per plugin (cache lookup, store, forget cleanup).
  registry_cache: Mutex<HashMap<RegistryUrlKey, npm_resolution::NpmRegistryResolution>>,
}

#[derive(Hash, PartialEq, Eq)]
struct RegistryUrlKey {
  package_name: String,
  start_dir: Option<PathBuf>,
}

impl<TEnvironment: Environment> ConcurrentPluginCacheManifest<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    let manifest = RwLock::new(read_manifest(&environment));
    Self {
      environment,
      manifest,
      registry_cache: Mutex::new(HashMap::new()),
    }
  }

  pub(super) fn resolve_registry(&self, package_name: &str, start_dir: Option<&Path>) -> npm_resolution::NpmRegistryResolution {
    let key = RegistryUrlKey {
      package_name: package_name.to_string(),
      start_dir: start_dir.map(|p| p.to_path_buf()),
    };
    if let Some(info) = self.registry_cache.lock().get(&key) {
      return info.clone();
    }
    // resolved outside the lock since it does file I/O.
    // a concurrent caller may compute the same value — harmless since the
    // result is deterministic for a given key.
    let info = npm_resolution::resolve_registry_for_package(package_name, start_dir, &self.environment);
    self.registry_cache.lock().insert(key, info.clone());
    info
  }

  pub(super) fn resolve_registry_url(&self, package_name: &str, start_dir: Option<&Path>) -> String {
    self.resolve_registry(package_name, start_dir).url
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
      PathSource::Npm(npm_source) => {
        // only versioned npm specifiers use the npm cache key —
        // unversioned ones are mapped to local paths before reaching here
        // (see `get_plugin_cache_item` / `manifest_source_for_forget`).
        // include the resolved registry (private vs public mirror) and the
        // specifier path (a single package can ship plugin.wasm and plugin.json)
        // so distinct specifiers don't share an entry.
        let Some(version) = npm_source.specifier.version.as_deref() else {
          bail!(
            "Internal error: cache key requested for unversioned npm specifier {} — this should have been mapped to a local path.",
            npm_source.specifier.display(),
          );
        };
        let start_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
        let registry = self.resolve_registry_url(&npm_source.specifier.name, start_dir);
        format!("npm:{}#{}@{}/{}", registry, npm_source.specifier.name, version, npm_source.specifier.path,)
      }
    })
  }
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
        "schemaVersion": 9,
        "wasmCacheVersion": WASMER_COMPILER_VERSION,
        "plugins": {
          "remote:https://plugins.dprint.dev/test.wasm": {
            "createdTime": 123456,
            "pluginKind": "Wasm",
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
        "schemaVersion": 9,
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
      "schemaVersion": 9,
      "wasmCacheVersion": WASMER_COMPILER_VERSION,
      "plugins": {
        "local:/test.wasm": {
          "createdTime": 123456,
          "fileHash": get_bytes_hash(&WASM_PLUGIN_BYTES),
          "pluginKind": "Wasm",
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
      "schemaVersion": 9,
      "wasmCacheVersion": WASMER_COMPILER_VERSION,
      "plugins": {
        "local:/test.wasm": {
          "createdTime": 123456,
          "fileHash": get_bytes_hash(&WASM_PLUGIN_0_1_0_BYTES),
          "pluginKind": "Wasm",
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
        "schemaVersion": 9,
        "wasmCacheVersion": WASMER_COMPILER_VERSION,
        "plugins": {}
      })
      .to_string(),
    );

    Ok(())
  }

  #[tokio::test]
  async fn forget_removes_npm_extract_dir() -> Result<()> {
    use dprint_core::plugins::PluginInfo;

    use crate::plugins::PluginCacheManifestItem;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    let specifier = NpmSpecifier {
      name: "@dprint/test".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(specifier.clone(), None),
      checksum: None,
    };

    // seed the manifest and the on-disk artifacts as if a previous resolve had run
    let extract_dir = environment.get_cache_dir().join("npm").join("registry.npmjs.org").join("@dprint__test@1.0.0");
    environment.mk_dir_all(&extract_dir).unwrap();
    environment.write_file(&extract_dir.join("plugin.wasm"), "fake").unwrap();
    let compiled_wasm_path = environment
      .get_cache_dir()
      .join("plugins")
      .join("test-plugin")
      .join(format!("1.0.0-{WASMER_COMPILER_VERSION}-x86_64"));
    environment.mk_dir_all(compiled_wasm_path.parent().unwrap()).unwrap();
    environment.write_file(&compiled_wasm_path, "compiled").unwrap();

    plugin_cache.manifest.add(
      &plugin_source.path_source,
      PluginCacheManifestItem {
        created_time: 0,
        file_hash: None,
        plugin_kind: Some(PluginKind::Wasm),
        info: PluginInfo {
          name: "test-plugin".to_string(),
          version: "1.0.0".to_string(),
          config_key: "test".to_string(),
          help_url: "help".to_string(),
          config_schema_url: "schema".to_string(),
          update_url: None,
        },
      },
    )?;

    plugin_cache.forget(&plugin_source).await?;

    assert_eq!(environment.path_exists(&extract_dir), false);
    assert_eq!(environment.path_exists(&compiled_wasm_path), false);
    Ok(())
  }

  #[tokio::test]
  async fn forget_unversioned_npm_removes_node_modules_manifest_entry() -> Result<()> {
    use dprint_core::plugins::PluginInfo;

    use crate::plugins::PluginCacheManifestItem;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    // simulate a node_modules layout so find_npm_plugin_local_path resolves
    let plugin_path = environment.cwd().join("node_modules").join("foo").join("plugin.wasm");
    environment.mk_dir_all(plugin_path.parent().unwrap()).unwrap();
    environment.write_file_bytes(&plugin_path, b"wasm").unwrap();
    let canonical = environment.canonicalize(&plugin_path).unwrap();

    // unversioned npm: resolve flow stores the manifest entry under the local
    // path key, so forget needs to translate to find it
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: None,
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let compiled_wasm_path = environment
      .get_cache_dir()
      .join("plugins")
      .join("foo-plugin")
      .join(format!("1.0.0-{WASMER_COMPILER_VERSION}-x86_64"));
    environment.mk_dir_all(compiled_wasm_path.parent().unwrap()).unwrap();
    environment.write_file(&compiled_wasm_path, "compiled").unwrap();

    let local_path_source = PathSource::new_local(canonical.clone());
    plugin_cache.manifest.add(
      &local_path_source,
      PluginCacheManifestItem {
        created_time: 0,
        file_hash: Some(0),
        plugin_kind: Some(PluginKind::Wasm),
        info: PluginInfo {
          name: "foo-plugin".to_string(),
          version: "1.0.0".to_string(),
          config_key: "foo".to_string(),
          help_url: "help".to_string(),
          config_schema_url: "schema".to_string(),
          update_url: None,
        },
      },
    )?;
    assert!(plugin_cache.manifest.get(&local_path_source)?.is_some());

    plugin_cache.forget(&plugin_source).await?;

    assert!(plugin_cache.manifest.get(&local_path_source)?.is_none());
    Ok(())
  }

  #[tokio::test]
  async fn forget_unversioned_npm_with_missing_node_modules_is_noop() -> Result<()> {
    // a user deletes node_modules between dprint runs. The resolver will then
    // call forget on the original unversioned npm reference. Without a local
    // path we have no manifest key to remove, so forget should be a no-op
    // instead of surfacing the internal "unversioned npm" cache-key error.
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "missing".to_string(),
          version: None,
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    plugin_cache.forget(&plugin_source).await?;
    Ok(())
  }

  #[tokio::test]
  async fn npm_cache_key_distinguishes_specifier_paths() -> Result<()> {
    use dprint_core::plugins::PluginInfo;

    use crate::plugins::PluginCacheManifestItem;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    let make_source = |path: &str| PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some("1.0.0".to_string()),
          path: path.to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let make_item = |name: &str| PluginCacheManifestItem {
      created_time: 0,
      file_hash: None,
      plugin_kind: Some(PluginKind::Wasm),
      info: PluginInfo {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        config_key: "test".to_string(),
        help_url: "help".to_string(),
        config_schema_url: "schema".to_string(),
        update_url: None,
      },
    };

    // same name@version, different plugin file → distinct cache entries
    let wasm_source = make_source("plugin.wasm");
    let json_source = make_source("plugin.json");
    plugin_cache.manifest.add(&wasm_source.path_source, make_item("foo-wasm"))?;
    plugin_cache.manifest.add(&json_source.path_source, make_item("foo-process"))?;
    assert_eq!(plugin_cache.manifest.get(&wasm_source.path_source)?.unwrap().info.name, "foo-wasm");
    assert_eq!(plugin_cache.manifest.get(&json_source.path_source)?.unwrap().info.name, "foo-process");

    Ok(())
  }

  #[tokio::test]
  async fn npm_cache_key_rejects_unversioned_npm_path_source() {
    // Unversioned npm specifiers are supposed to be mapped to a local path
    // before they reach the cache-key code. If that mapping is ever skipped,
    // we should surface a loud error rather than silently bucket all
    // unversioned specifiers into one "latest" entry.
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment);

    let path_source = PathSource::new_npm(
      NpmSpecifier {
        name: "foo".to_string(),
        version: None,
        path: "plugin.wasm".to_string(),
      },
      None,
    );

    let err = plugin_cache.manifest.get(&path_source).unwrap_err();
    assert!(
      err.to_string().contains("unversioned npm specifier"),
      "expected unversioned-npm error, got: {}",
      err
    );
  }

  #[tokio::test]
  async fn npm_cache_key_includes_registry() -> Result<()> {
    use dprint_core::plugins::PluginInfo;

    use crate::plugins::PluginCacheManifestItem;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some("1.0.0".to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let make_item = |name: &str| PluginCacheManifestItem {
      created_time: 0,
      file_hash: None,
      plugin_kind: Some(PluginKind::Wasm),
      info: PluginInfo {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        config_key: "test".to_string(),
        help_url: "help".to_string(),
        config_schema_url: "schema".to_string(),
        update_url: None,
      },
    };

    // entry resolved against the default registry
    plugin_cache.manifest.add(&plugin_source.path_source, make_item("public-foo"))?;

    // a second cache (new dprint invocation) pointed at a private registry
    // via env should not see the public entry and should store its own
    environment.set_env_var("NPM_CONFIG_REGISTRY", Some("https://private.example.com"));
    let private_cache = PluginCache::new(environment.clone());
    assert!(private_cache.manifest.get(&plugin_source.path_source)?.is_none());
    private_cache.manifest.add(&plugin_source.path_source, make_item("private-foo"))?;
    assert_eq!(private_cache.manifest.get(&plugin_source.path_source)?.unwrap().info.name, "private-foo");

    // and switching back (yet another invocation) still finds the original entry
    environment.set_env_var("NPM_CONFIG_REGISTRY", None);
    let public_cache = PluginCache::new(environment.clone());
    assert_eq!(public_cache.manifest.get(&plugin_source.path_source)?.unwrap().info.name, "public-foo");

    Ok(())
  }

  #[tokio::test]
  async fn npm_registry_resolve_caches_and_avoids_second_fetch() -> Result<()> {
    use crate::test_helpers::create_test_npm_tarball;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();

    let packument = serde_json::json!({
      "versions": {
        "1.0.0": {
          "dist": { "tarball": "https://registry.npmjs.org/some-plugin/-/some-plugin-1.0.0.tgz" }
        }
      }
    });
    let packument_url = "https://registry.npmjs.org/some-plugin";
    let tarball_url = "https://registry.npmjs.org/some-plugin/-/some-plugin-1.0.0.tgz";
    environment.add_remote_file_bytes(packument_url, packument.to_string().into_bytes());
    environment.add_remote_file_bytes(tarball_url, create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]));

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "some-plugin".to_string(),
          version: Some("1.0.0".to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let cache_item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(cache_item.plugin_kind, PluginKind::Wasm);
    assert_eq!(cache_item.info.name, "test-plugin");
    assert_eq!(cache_item.info.version, "0.2.0");

    // tarball extracted under registry-host segment
    let extract_dir = environment.get_cache_dir().join("npm").join("registry.npmjs.org").join("some-plugin@1.0.0");
    assert!(environment.path_exists(&extract_dir.join("plugin.wasm")));

    // drain the wasm-compile log so it doesn't fail the drop check
    let _ = environment.take_stderr_messages();

    // poison the remote endpoints — a second resolve must hit the cache, not refetch
    environment.add_remote_file_error(packument_url, "must not be fetched again");
    environment.add_remote_file_error(tarball_url, "must not be fetched again");
    let cached = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(cached.info.name, "test-plugin");

    Ok(())
  }

  #[tokio::test]
  async fn npm_node_modules_resolve_wasm_walks_up_from_subdir() -> Result<()> {
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    // place the package at /node_modules/foo, but run from a nested cwd to
    // exercise the ancestor walk
    let pkg_dir = "/node_modules/foo";
    environment.mk_dir_all(pkg_dir).unwrap();
    environment
      .write_file_bytes(&PathBuf::from(pkg_dir).join("plugin.wasm"), WASM_PLUGIN_BYTES)
      .unwrap();
    environment.mk_dir_all("/project/sub").unwrap();
    environment.set_cwd("/project/sub");

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: None,
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let cache_item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(cache_item.plugin_kind, PluginKind::Wasm);
    assert_eq!(cache_item.info.name, "test-plugin");
    let _ = environment.take_stderr_messages();
    Ok(())
  }

  #[tokio::test]
  async fn npm_node_modules_resolve_missing_package_errors() -> Result<()> {
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "nope".to_string(),
          version: None,
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let err = match plugin_cache.get_plugin_cache_item(&plugin_source).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    // the inner error names the package; the wrapping context names the
    // user-typed npm specifier so it's easy to map an error back to the config
    let chained = format!("{err:#}");
    assert!(chained.contains("Resolving npm:nope"), "expected npm-spec context, got: {chained}");
    assert!(chained.contains("Could not find nope in node_modules"), "got: {chained}");
    Ok(())
  }

  #[tokio::test]
  async fn npm_node_modules_resolve_missing_plugin_file_errors() -> Result<()> {
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();
    // package exists but the requested plugin file does not
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file(&PathBuf::from("/node_modules/foo/package.json"), "{}").unwrap();

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: None,
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let err = match plugin_cache.get_plugin_cache_item(&plugin_source).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let chained = format!("{err:#}");
    assert!(chained.contains("Could not find plugin.wasm"), "got: {chained}");
    Ok(())
  }

  #[tokio::test]
  async fn npm_registry_tarball_rejects_path_traversal_entries() -> Result<()> {
    use crate::test_helpers::create_test_npm_tarball_raw_paths;
    use crate::utils::NpmSpecifier;

    let environment = TestEnvironment::new();

    let packument = serde_json::json!({
      "versions": {
        "1.0.0": {
          "dist": { "tarball": "https://registry.npmjs.org/evil/-/evil-1.0.0.tgz" }
        }
      }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/evil", packument.to_string().into_bytes());
    // a tarball with an entry that, after stripping the wrapper, escapes the extract dir.
    // built via raw-path helper because tar::Header::set_path rejects `..`.
    let tarball = create_test_npm_tarball_raw_paths(&[("package/plugin.wasm", b"good"), ("package/../../../etc/passwd", b"pwned")]);
    environment.add_remote_file_bytes("https://registry.npmjs.org/evil/-/evil-1.0.0.tgz", tarball);

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "evil".to_string(),
          version: Some("1.0.0".to_string()),
          path: "plugin.wasm".to_string(),
        },
        None,
      ),
      checksum: None,
    };

    let err = match plugin_cache.get_plugin_cache_item(&plugin_source).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    assert!(err.to_string().contains("outside output directory"), "got: {}", err);
    // and the escaping file must not have been written anywhere
    assert!(!environment.path_exists("/etc/passwd"));
    Ok(())
  }

  #[tokio::test]
  async fn local_process_plugin_hash_invalidates_when_local_archive_changes() {
    // A process plugin whose plugin.json references a *local* archive (relative
    // path / file://) is at risk of stale cache hits if the archive is edited
    // without touching plugin.json. The cache invalidation hash must mix the
    // archive's bytes in so this is detected.
    let environment = TestEnvironment::new();

    // any non-empty bytes work — the helper hashes them and never verifies the
    // checksum field
    let zip_bytes_v1: &[u8] = b"zip-v1";
    let zip_bytes_v2: &[u8] = b"zip-v2-different";
    let zip_checksum = crate::utils::get_sha256_checksum(zip_bytes_v1);
    let plugin_json = format!(
      r#"{{
  "schemaVersion": 2,
  "name": "p",
  "version": "0.1.0",
  "linux-x86_64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }},
  "linux-aarch64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }},
  "darwin-x86_64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }},
  "darwin-aarch64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }},
  "windows-x86_64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }},
  "windows-aarch64": {{ "reference": "./bin.zip", "checksum": "{zip_checksum}" }}
}}"#,
    );
    let plugin_json_path = PathBuf::from("/pkg/plugin.json");
    let archive_path = PathBuf::from("/pkg/bin.zip");
    environment.mk_dir_all("/pkg").unwrap();
    environment.write_file_bytes(&plugin_json_path, plugin_json.as_bytes()).unwrap();
    environment.write_file_bytes(&archive_path, zip_bytes_v1).unwrap();

    let source = PathSource::new_local(environment.canonicalize(&plugin_json_path).unwrap());
    let plugin_bytes = environment.read_file_bytes(&plugin_json_path).unwrap();

    let hash_v1 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Process, None, &environment);

    // editing the archive must invalidate the hash — this is the regression
    environment.write_file_bytes(&archive_path, zip_bytes_v2).unwrap();
    let hash_v2 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Process, None, &environment);
    assert_ne!(hash_v1, hash_v2, "editing local archive must change the cache invalidation hash");

    // with a pre_resolved_tarball the per-platform archive is fetched from
    // npm (content-addressed by plugin.json's checksum), so the local file
    // on disk is irrelevant and must NOT factor into the hash
    let dummy_tarball = npm_resolution::PreResolvedProcessPluginTarball {
      name: "p".to_string(),
      version: "0.1.0".to_string(),
      tarball_bytes: Vec::new(),
      executable_sub_path: String::new(),
    };
    let with_tarball_v1 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Process, Some(&dummy_tarball), &environment);
    environment.write_file_bytes(&archive_path, zip_bytes_v1).unwrap();
    let with_tarball_v2 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Process, Some(&dummy_tarball), &environment);
    assert_eq!(
      with_tarball_v1, with_tarball_v2,
      "with pre_resolved_tarball, local archive bytes must not affect the hash"
    );

    // wasm plugins: archive bytes shouldn't factor in either
    let wasm_v1 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Wasm, None, &environment);
    environment.write_file_bytes(&archive_path, zip_bytes_v2).unwrap();
    let wasm_v2 = compute_local_plugin_file_hash(&source, &plugin_bytes, PluginKind::Wasm, None, &environment);
    assert_eq!(wasm_v1, wasm_v2, "wasm plugins should not mix in archive bytes");
  }

  #[tokio::test]
  async fn should_resolve_redirected_process_plugin_with_relative_urls() -> Result<()> {
    let environment = TestEnvironment::new();

    // create a plugin.json that uses a relative path for the zip reference
    let zip_bytes = &*crate::test_helpers::PROCESS_PLUGIN_ZIP_BYTES;
    let zip_checksum = crate::test_helpers::PROCESS_PLUGIN_ZIP_CHECKSUM.as_str();
    let plugin_json = format!(
      r#"{{
  "schemaVersion": 2,
  "name": "test-process-plugin",
  "version": "0.1.0",
  "linux-x86_64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }},
  "linux-aarch64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }},
  "darwin-x86_64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }},
  "darwin-aarch64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }},
  "windows-x86_64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }},
  "windows-aarch64": {{ "reference": "./test-process-plugin.zip", "checksum": "{zip_checksum}" }}
}}"#,
    );

    // host the plugin.json at the CDN (redirect target)
    let cdn_plugin_url = "https://cdn.example.com/plugins/v1/test-process.json";
    environment.add_remote_file_bytes(cdn_plugin_url, plugin_json.as_bytes().to_vec());
    // host the zip relative to the plugin.json on the CDN
    environment.add_remote_file_bytes("https://cdn.example.com/plugins/v1/test-process-plugin.zip", zip_bytes.to_vec());
    // the original URL redirects to the CDN
    let original_url = "https://plugins.example.com/test-process.json";
    environment.add_remote_file_redirect(original_url, cdn_plugin_url);

    let plugin_json_checksum = crate::utils::get_sha256_checksum(plugin_json.as_bytes());
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference {
      path_source: PathSource::new_remote(url::Url::parse(original_url).unwrap()),
      checksum: Some(plugin_json_checksum),
    };
    let cache_item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(cache_item.info.name, "test-process-plugin");
    assert_eq!(cache_item.info.version, "0.1.0");

    Ok(())
  }
}
