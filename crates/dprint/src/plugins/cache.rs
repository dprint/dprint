use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use dprint_core::plugins::PluginInfo;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;

use super::cache_fs_locks::CacheFsLockGuard;
use super::cache_fs_locks::CacheFsLockPool;
use super::cache_meta::LocalStamp;
use super::cache_meta::PluginCacheMeta;
use super::cache_meta::current_signature;
use super::cache_meta::entry_hash;
use super::cache_meta::plugins_dir;
use super::cache_meta::process_dir_path;
use super::cache_meta::read_meta;
use super::cache_meta::remove_entry;
use super::cache_meta::to_unix_millis;
use super::cache_meta::wasm_artifact_path;
use super::cache_meta::write_meta;
use super::implementations::SetupPluginDest;
use super::implementations::SetupPluginOptions;
use super::implementations::get_process_plugin_os_path;
use super::implementations::parse_process_plugin_file;
use super::implementations::setup_plugin;
use super::npm_resolution;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::PluginSourceReference;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::get_sha256_checksum;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;

pub struct PluginCacheItem {
  pub file_path: PathBuf,
  pub info: PluginInfo,
  pub plugin_kind: PluginKind,
}

/// What [`PluginCache::resolve_npm_for_add`] resolved: the plugin kind, the
/// plugin file path within the package (detected for a pathless specifier), and
/// the tarball checksum — for the caller to write into the config entry.
pub struct NpmAddResolution {
  pub plugin_kind: PluginKind,
  pub path: String,
  pub checksum: String,
}

/// Inputs to [`PluginCache::setup_and_store`]. `hash`/`cache_key` identify the
/// cache entry; the rest is the resolved plugin to set up.
struct SetupAndStoreOptions<'a> {
  hash: &'a str,
  cache_key: &'a str,
  resolved_source: &'a PathSource,
  file_bytes: Vec<u8>,
  plugin_kind: PluginKind,
  pre_resolved_tarball: Option<npm_resolution::PreResolvedProcessPluginTarball>,
  local_stamps: Option<Vec<LocalStamp>>,
}

/// Inputs to [`PluginCache::verify_and_store_plugin`]: the resolved bytes plus
/// the cache identity, ready to verify and store.
struct VerifyAndStoreOptions<'a> {
  source_reference: &'a PluginSourceReference,
  cache_key: &'a str,
  hash: &'a str,
  file_bytes: Vec<u8>,
  resolved_source: PathSource,
  primary_stamp: Option<LocalStamp>,
}

/// The default plugin file name within an npm package for each kind.
fn default_plugin_path(kind: PluginKind) -> &'static str {
  match kind {
    PluginKind::Wasm => "plugin.wasm",
    PluginKind::Process => "plugin.json",
  }
}

/// On-disk cache of set-up plugins.
///
/// Each plugin gets a flat pair of files under `<cache>/plugins/` keyed by a
/// hash of its source: a `<hash>.json` sidecar ([`PluginCacheMeta`]) plus the
/// artifact itself (`<hash>.cwasm` for wasm, a `<hash>/` extract dir for
/// process plugins). There is no global manifest, so caching or forgetting one
/// plugin never rewrites state for the others.
pub struct PluginCache<TEnvironment: Environment> {
  environment: TEnvironment,
  fs_locks: CacheFsLockPool<TEnvironment>,
  /// Memoized npm registry resolutions keyed on (package name, config dir).
  /// Resolving via `.npmrc` walks the directory tree, so the same key is hit
  /// multiple times per plugin (cache lookup, store, forget cleanup).
  registry_cache: Mutex<HashMap<RegistryUrlKey, npm_resolution::NpmRegistryResolution>>,
}

impl<TEnvironment> PluginCache<TEnvironment>
where
  TEnvironment: Environment,
{
  pub fn new(environment: TEnvironment) -> Self {
    PluginCache {
      fs_locks: CacheFsLockPool::new(environment.clone()),
      registry_cache: Mutex::new(HashMap::new()),
      environment,
    }
  }

  pub async fn forget_and_recreate(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    // lock on the same key the inner forget+resolve will use so this whole
    // forget-then-recreate sequence appears atomic to other processes. For
    // unversioned npm that's the resolved node_modules path (see
    // `cache_source_for_forget`); fall back to the source itself otherwise.
    let lock_source = self
      .cache_source_for_forget(&source_reference.path_source)
      .unwrap_or_else(|| source_reference.path_source.clone());
    let _setup_guard = self.fs_locks.lock(&lock_source).await;
    self.forget(source_reference).await?;
    self.get_plugin_cache_item(source_reference).await
  }

  pub async fn forget(&self, source_reference: &PluginSourceReference) -> Result<()> {
    // Unversioned npm specifiers cache under their resolved local node_modules
    // path, not under the npm path. Translate so we target the same key the
    // resolve flow stored under. If the package is no longer in node_modules we
    // have no path to key on, so there's nothing to forget.
    let Some(cache_source) = self.cache_source_for_forget(&source_reference.path_source) else {
      return Ok(());
    };
    // Lock on the same key the resolve flow uses (`get_local_plugin` for
    // unversioned npm), not the original npm PathSource — otherwise a
    // concurrent process resolving the same plugin would lock on a different
    // key and we'd race between remove_entry here and setup over there.
    let _setup_guard = self.fs_locks.lock(&cache_source).await;
    let cache_key = self.compute_cache_key(&cache_source)?;
    let hash = entry_hash(&cache_key, &self.environment);
    remove_entry(&hash, &self.environment);

    // also remove the npm tarball extract dir for versioned npm specifiers
    if let PathSource::Npm(npm_source) = &source_reference.path_source
      && let Some(version) = &npm_source.specifier.version
    {
      let start_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
      let registry = self.resolve_registry_url(&npm_source.specifier.name, start_dir);
      let registry_segment = npm_resolution::registry_dir_segment(&registry);
      let extract_dir = npm_resolution::get_npm_extract_dir(&registry_segment, &npm_source.specifier.name, version, &self.environment);
      self.environment.try_remove_dir_all(&extract_dir);
    }

    Ok(())
  }

  /// Returns the `PathSource` whose cache key matches how the resolve flow
  /// stored the entry. For unversioned npm this is the resolved local
  /// node_modules path; for everything else it's the source itself. Returns
  /// `None` for an unversioned npm whose package is no longer in node_modules
  /// — without a local path we have no key to look up, so there's nothing to
  /// forget.
  fn cache_source_for_forget(&self, path_source: &PathSource) -> Option<PathSource> {
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
    match &source_reference.path_source {
      PathSource::Remote(_) | PathSource::Local(_) => self.get_plugin(source_reference).await,
      PathSource::Npm(npm_source) => {
        if npm_source.specifier.version.is_some() {
          self.get_npm_registry_plugin(source_reference, npm_source).await
        } else {
          // resolve to a local path, then delegate to the local plugin caching
          // so file-change detection works the same as any local plugin
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
        }
      }
    }
  }

  async fn get_npm_registry_plugin(&self, source_reference: &PluginSourceReference, npm_source: &crate::utils::NpmPathSource) -> Result<PluginCacheItem> {
    let (cache_key, hash, _setup_guard) = match self.lookup_or_lock(&source_reference.path_source).await? {
      CacheLookup::Hit(item) => return Ok(item),
      CacheLookup::Miss { cache_key, hash, guard } => (cache_key, hash, guard),
    };

    let specifier = &npm_source.specifier;
    let checksum = source_reference.checksum.as_deref();
    let base_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
    let registry = self.resolve_registry(&specifier.name, base_dir);
    let resolved = npm_resolution::resolve_npm_from_registry(
      npm_resolution::ResolveNpmRegistryOptions {
        specifier,
        checksum,
        detect_path: false,
        establish_checksum: false,
        registry: &registry,
        config_dir: base_dir,
      },
      &self.environment,
    )
    .await?;

    // npm-versioned content is pinned by name@version, so there are no local
    // stamps — a present entry is always a hit.
    self
      .setup_and_store(SetupAndStoreOptions {
        hash: &hash,
        cache_key: &cache_key,
        resolved_source: &resolved.local_path,
        file_bytes: resolved.plugin_bytes,
        plugin_kind: resolved.plugin_kind,
        pre_resolved_tarball: resolved.pre_resolved_tarball,
        local_stamps: None,
      })
      .await
      .with_context(|| format!("Setting up {}", specifier.display()))
  }

  /// Sets up a versioned npm plugin for `dprint add`: resolves the plugin file
  /// (detecting it for a pathless specifier), computes the tarball checksum, and
  /// warms the plugin cache so the first `dprint fmt` is a hit. Returns the
  /// resolved path + checksum for the caller to write into config. Reuses the
  /// checksum sidecar to skip the download on a repeat add.
  pub async fn resolve_npm_for_add(
    &self,
    specifier: &NpmSpecifier,
    path_was_explicit: bool,
    base_dir: Option<&CanonicalizedPathBuf>,
  ) -> Result<NpmAddResolution> {
    let version = specifier
      .version
      .as_deref()
      .ok_or_else(|| anyhow::anyhow!("Internal error: resolve_npm_for_add requires a versioned specifier"))?;
    let base_dir_ref = base_dir.map(|d| d.as_ref());

    // a prior add/format cached this version's checksum + kind — reuse it
    // instead of re-downloading the tarball.
    if let Some(entry) = npm_resolution::read_npm_add_cache(&specifier.name, version, base_dir_ref, &self.environment) {
      let path = if path_was_explicit {
        specifier.path.clone()
      } else {
        default_plugin_path(entry.plugin_kind).to_string()
      };
      return Ok(NpmAddResolution {
        plugin_kind: entry.plugin_kind,
        path,
        checksum: entry.tarball_sha256,
      });
    }

    let registry = self.resolve_registry(&specifier.name, base_dir_ref);
    let resolved = npm_resolution::resolve_npm_from_registry(
      npm_resolution::ResolveNpmRegistryOptions {
        specifier,
        checksum: None,
        detect_path: !path_was_explicit,
        establish_checksum: true,
        registry: &registry,
        config_dir: base_dir_ref,
      },
      &self.environment,
    )
    .await?;
    let checksum = resolved
      .tarball_checksum
      .clone()
      .ok_or_else(|| anyhow::anyhow!("Internal error: registry resolve did not compute a checksum"))?;

    // warm the compiled cache under the resolved path's key so the first
    // `dprint fmt` is a hit (the key ignores the checksum, so writing it to
    // config afterward still matches).
    let resolved_specifier = NpmSpecifier {
      name: specifier.name.clone(),
      version: Some(version.to_string()),
      path: resolved.resolved_path.clone(),
    };
    let path_source = PathSource::new_npm(resolved_specifier, base_dir.cloned());
    if let CacheLookup::Miss { cache_key, hash, .. } = self.lookup_or_lock(&path_source).await? {
      self
        .setup_and_store(SetupAndStoreOptions {
          hash: &hash,
          cache_key: &cache_key,
          resolved_source: &resolved.local_path,
          file_bytes: resolved.plugin_bytes,
          plugin_kind: resolved.plugin_kind,
          pre_resolved_tarball: resolved.pre_resolved_tarball,
          local_stamps: None,
        })
        .await
        .with_context(|| format!("Setting up {}", specifier.display()))?;
    }

    Ok(NpmAddResolution {
      plugin_kind: resolved.plugin_kind,
      path: resolved.resolved_path,
      checksum,
    })
  }

  /// Downloads a remote plugin for `dprint add`, computes its checksum, and
  /// warms the plugin cache so the first `dprint fmt` is a hit. Returns the
  /// checksum for the caller to write into config.
  pub async fn resolve_remote_for_add(&self, source_reference: &PluginSourceReference) -> Result<String> {
    let remote = match &source_reference.path_source {
      PathSource::Remote(remote) => remote,
      _ => bail!("Internal error: resolve_remote_for_add requires a remote source"),
    };
    let plugin_kind = source_reference
      .plugin_kind()
      .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;
    let (resolved_url, file) = self.environment.download_file_err_404(&remote.url, None).await?;
    let file_bytes = file.content;
    let checksum = get_sha256_checksum(&file_bytes);

    if let CacheLookup::Miss { cache_key, hash, .. } = self.lookup_or_lock(&source_reference.path_source).await? {
      let resolved_source = PathSource::new_remote(resolved_url.into_owned());
      self
        .setup_and_store(SetupAndStoreOptions {
          hash: &hash,
          cache_key: &cache_key,
          resolved_source: &resolved_source,
          file_bytes,
          plugin_kind,
          pre_resolved_tarball: None,
          local_stamps: None,
        })
        .await
        .with_context(|| format!("Setting up {}", source_reference.display()))?;
    }

    Ok(checksum)
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
      .ok_or_else(|| anyhow::anyhow!("Expected local path for npm node_modules plugin"))?
      .clone();

    let (cache_key, hash, _setup_guard) = match self.lookup_or_lock(&source_reference.path_source).await? {
      CacheLookup::Hit(item) => return Ok(item),
      CacheLookup::Miss { cache_key, hash, guard } => (cache_key, hash, guard),
    };

    // stamp the source file *before* reading it, so an edit during our read is
    // caught next run (the stored stamp predates the bytes we compile).
    let primary_stamp = self.stamp_for(&local_path);
    let file_bytes = self.environment.read_file_bytes(&local_path)?;
    let plugin_kind = source_reference
      .plugin_kind()
      .ok_or_else(|| anyhow::anyhow!("Could not determine plugin kind for {}", source_reference.display()))?;

    // for npm-resolved process plugins the per-platform binary comes from the
    // npm registry (verified against plugin.json's checksum), so the plugin
    // file's bytes alone fully determine the cached output. For process plugins
    // whose per-platform reference points to a *local* archive (relative path /
    // file://), `build_local_stamps` also stamps that archive so editing it
    // invalidates the cache.
    let local_stamps = self.build_local_stamps(
      primary_stamp,
      &source_reference.path_source,
      &file_bytes,
      plugin_kind,
      pre_resolved_tarball.as_ref(),
    );
    self
      .setup_and_store(SetupAndStoreOptions {
        hash: &hash,
        cache_key: &cache_key,
        resolved_source: &source_reference.path_source,
        file_bytes,
        plugin_kind,
        pre_resolved_tarball,
        local_stamps,
      })
      .await
  }

  async fn get_plugin(&self, source_reference: &PluginSourceReference) -> Result<PluginCacheItem> {
    let (cache_key, hash, _setup_guard) = match self.lookup_or_lock(&source_reference.path_source).await? {
      CacheLookup::Hit(item) => return Ok(item),
      CacheLookup::Miss { cache_key, hash, guard } => (cache_key, hash, guard),
    };

    // stamp a local source *before* reading it, so an edit during our read is
    // caught next run (the stored stamp predates the bytes we compile). Remote
    // sources can't change underneath us and don't get stamps.
    let primary_stamp = source_reference.path_source.maybe_local_path().and_then(|p| self.stamp_for(p));

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

    self
      .verify_and_store_plugin(VerifyAndStoreOptions {
        source_reference,
        cache_key: &cache_key,
        hash: &hash,
        file_bytes,
        resolved_source,
        primary_stamp,
      })
      .await
  }

  /// Shared tail of plugin setup once the file bytes are in hand: verifies the
  /// checksum, computes local stamps, and stores the cached entry.
  async fn verify_and_store_plugin(&self, options: VerifyAndStoreOptions<'_>) -> Result<PluginCacheItem> {
    let VerifyAndStoreOptions {
      source_reference,
      cache_key,
      hash,
      file_bytes,
      resolved_source,
      primary_stamp,
    } = options;
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

    // a local source never redirects, so resolved_source == the stamped source;
    // a remote redirect stays remote and gets no stamps.
    let local_stamps = if resolved_source.is_local() {
      self.build_local_stamps(primary_stamp, &resolved_source, &file_bytes, plugin_kind, None)
    } else {
      None
    };

    self
      .setup_and_store(SetupAndStoreOptions {
        hash,
        cache_key,
        resolved_source: &resolved_source,
        file_bytes,
        plugin_kind,
        pre_resolved_tarball: None,
        local_stamps,
      })
      .await
  }

  /// Probes the cache for `source`; on a miss, acquires the per-source setup
  /// lock and probes again (the standard double-checked pattern). Centralizes
  /// the invariant that the lock key matches the key the entry is stored under.
  /// On `Miss` the returned guard must be held for the duration of the setup.
  async fn lookup_or_lock(&self, source: &PathSource) -> Result<CacheLookup<TEnvironment>> {
    let cache_key = self.compute_cache_key(source)?;
    let hash = entry_hash(&cache_key, &self.environment);
    if let Some(item) = self.cached_item(source, &hash, &cache_key) {
      return Ok(CacheLookup::Hit(item));
    }
    let guard = self.fs_locks.lock(source).await;
    // re-check after acquiring the lock in case another process just set it up
    if let Some(item) = self.cached_item(source, &hash, &cache_key) {
      return Ok(CacheLookup::Hit(item));
    }
    Ok(CacheLookup::Miss { cache_key, hash, guard })
  }

  /// Sets up a freshly resolved plugin into the flat cache layout, writes its
  /// sidecar, and returns the cache item. Overwrites any existing entry for the
  /// same hash (e.g. a changed local file), so no explicit forget is needed.
  async fn setup_and_store(&self, options: SetupAndStoreOptions<'_>) -> Result<PluginCacheItem> {
    let SetupAndStoreOptions {
      hash,
      cache_key,
      resolved_source,
      file_bytes,
      plugin_kind,
      pre_resolved_tarball,
      local_stamps,
    } = options;
    self.environment.mk_dir_all(plugins_dir(&self.environment))?;
    let dest = SetupPluginDest {
      wasm_file_path: wasm_artifact_path(hash, &self.environment),
      process_dir_path: process_dir_path(hash, &self.environment),
    };
    let setup_result = setup_plugin(
      SetupPluginOptions {
        resolved_source,
        file_bytes,
        plugin_kind,
        pre_resolved_tarball,
        dest: &dest,
      },
      &self.environment,
    )
    .await?;

    let meta = PluginCacheMeta {
      source: cache_key.to_string(),
      signature: current_signature(&self.environment),
      plugin_kind,
      created_time: self.environment.get_time_secs(),
      info: setup_result.plugin_info.clone(),
      executable_sub_path: setup_result.executable_sub_path,
      local_stamps,
    };
    write_meta(hash, &meta, &self.environment)?;

    Ok(PluginCacheItem {
      file_path: setup_result.file_path,
      info: setup_result.plugin_info,
      plugin_kind,
    })
  }

  /// Reads the cached entry for `hash`, validating it belongs to `cache_key`
  /// (collision guard) and, for local sources, that the stamped source files
  /// are unchanged. Returns `None` on any miss.
  fn cached_item(&self, source: &PathSource, hash: &str, cache_key: &str) -> Option<PluginCacheItem> {
    let meta = read_meta(hash, &self.environment)?;
    if meta.source != cache_key {
      // different source hashed to the same filename — treat as a miss and let
      // the caller overwrite it
      return None;
    }
    if source.is_local() && !self.local_stamps_match(&meta) {
      return None;
    }
    Some(PluginCacheItem {
      file_path: meta.artifact_file_path(hash, &self.environment),
      info: meta.info,
      plugin_kind: meta.plugin_kind,
    })
  }

  /// Whether every stamped local source file still matches (size + mtime). A
  /// missing or empty stamp set means we can't vouch for it → treat as changed.
  fn local_stamps_match(&self, meta: &PluginCacheMeta) -> bool {
    let Some(stamps) = &meta.local_stamps else {
      return false;
    };
    if stamps.is_empty() {
      return false;
    }
    stamps.iter().all(|stamp| {
      self
        .file_size_and_mtime(&stamp.path)
        .map(|(len, modified)| len == stamp.len && to_unix_millis(modified) == stamp.modified_ms)
        .unwrap_or(false)
    })
  }

  /// Builds the change-detection stamps for a local source from a `primary`
  /// stamp captured *before* its bytes were read (see the call sites). For a
  /// process plugin whose per-platform archive is a *local* file, the archive is
  /// stamped too. Returns `None` if `primary` is absent (the file couldn't be
  /// stat'd), which forces a re-setup next time.
  fn build_local_stamps(
    &self,
    primary: Option<LocalStamp>,
    source: &PathSource,
    plugin_bytes: &[u8],
    plugin_kind: PluginKind,
    pre_resolved_tarball: Option<&npm_resolution::PreResolvedProcessPluginTarball>,
  ) -> Option<Vec<LocalStamp>> {
    let mut stamps = vec![primary?];
    if plugin_kind == PluginKind::Process
      && pre_resolved_tarball.is_none()
      && let Some(archive_path) = self.resolve_local_per_platform_archive_path(source, plugin_bytes)
      && let Some(stamp) = self.stamp_for(&archive_path)
    {
      stamps.push(stamp);
    }
    Some(stamps)
  }

  fn stamp_for(&self, path: impl AsRef<Path>) -> Option<LocalStamp> {
    let (len, modified) = self.file_size_and_mtime(&path)?;
    Some(LocalStamp {
      path: path.as_ref().to_string_lossy().into_owned(),
      len,
      modified_ms: to_unix_millis(modified),
    })
  }

  /// Size + modification time of a local file via sys_traits, or `None` if it
  /// can't be stat'd. The cheap stat that backs local cache-change detection.
  fn file_size_and_mtime(&self, path: impl AsRef<Path>) -> Option<(u64, SystemTime)> {
    let metadata = self.environment.fs_metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
  }

  /// If `plugin_bytes` is a parseable process-plugin manifest whose per-platform
  /// reference resolves to a local file (relative path / `file://`), returns
  /// that file's path. `None` for non-local references, parse failures, or a
  /// missing platform entry — callers fall back to a primary-only stamp and the
  /// real setup surfaces any error with better context.
  fn resolve_local_per_platform_archive_path(&self, source: &PathSource, plugin_bytes: &[u8]) -> Option<PathBuf> {
    let plugin_file = parse_process_plugin_file(plugin_bytes).ok()?;
    let os_path = get_process_plugin_os_path(&plugin_file, &self.environment).ok()?;
    // an `npm:` reference is handled via pre_resolved_tarball; an http(s) one is
    // fetched fresh during setup. we only stamp *local* references.
    if os_path.reference.starts_with("npm:") || os_path.reference.starts_with("http://") || os_path.reference.starts_with("https://") {
      return None;
    }
    let resolved = resolve_url_or_file_path_to_path_source(&os_path.reference, &source.parent(), &self.environment).ok()?;
    match resolved {
      PathSource::Local(local) => Some(local.path.into_path_buf()),
      _ => None,
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
    // resolved outside the lock since it does file I/O. a concurrent caller may
    // compute the same value — harmless since the result is deterministic.
    let info = npm_resolution::resolve_registry_for_package(package_name, start_dir, &self.environment);
    self.registry_cache.lock().insert(key, info.clone());
    info
  }

  pub(super) fn resolve_registry_url(&self, package_name: &str, start_dir: Option<&Path>) -> String {
    self.resolve_registry(package_name, start_dir).url
  }

  /// The stable cache key string for a source (`remote:<url>`, `local:<path>`,
  /// or `npm:<registry>#<name>@<ver>/<path>`). Hashed into the entry's filename
  /// and stored in the sidecar for collision detection.
  ///
  /// Entries are keyed purely per-source: the same plugin referenced two ways
  /// (e.g. a remote url and an npm specifier) is cached twice, by design — the
  /// flat layout trades a little duplicate disk for not needing to know a
  /// plugin's name@version before setting it up.
  fn compute_cache_key(&self, path_source: &PathSource) -> Result<String> {
    Ok(match path_source {
      PathSource::Remote(remote_source) => format!("remote:{}", remote_source.url.as_str()),
      PathSource::Local(local_source) => {
        let absolute_path = self.environment.canonicalize(&local_source.path)?;
        format!("local:{}", absolute_path.to_string_lossy())
      }
      PathSource::Npm(npm_source) => {
        // only versioned npm specifiers use the npm cache key — unversioned ones
        // are mapped to local paths before reaching here (see
        // `get_plugin_cache_item` / `cache_source_for_forget`). include the
        // resolved registry (private vs public mirror) and the specifier path (a
        // single package can ship plugin.wasm and plugin.json) so distinct
        // specifiers don't share an entry.
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

#[derive(Hash, PartialEq, Eq)]
struct RegistryUrlKey {
  package_name: String,
  start_dir: Option<PathBuf>,
}

/// Result of [`PluginCache::lookup_or_lock`]: either a ready cache hit, or a
/// miss carrying the key/hash plus the held setup lock so the caller can set up.
enum CacheLookup<TEnvironment: Environment> {
  Hit(PluginCacheItem),
  Miss {
    cache_key: String,
    hash: String,
    guard: CacheFsLockGuard<TEnvironment>,
  },
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::plugins::PluginSourceReference;
  use crate::test_helpers::WASM_PLUGIN_0_1_0_BYTES;
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use crate::utils::NpmSpecifier;
  use anyhow::Result;
  use pretty_assertions::assert_eq;
  use std::path::PathBuf;

  fn make_wasm_meta(cache_key: &str, name: &str, version: &str, environment: &TestEnvironment) -> PluginCacheMeta {
    PluginCacheMeta {
      source: cache_key.to_string(),
      signature: current_signature(environment),
      plugin_kind: PluginKind::Wasm,
      created_time: 0,
      info: PluginInfo {
        name: name.to_string(),
        version: version.to_string(),
        config_key: "test".to_string(),
        help_url: "help".to_string(),
        config_schema_url: "schema".to_string(),
        update_url: None,
      },
      executable_sub_path: None,
      local_stamps: None,
    }
  }

  #[tokio::test]
  async fn should_download_remote_file() -> Result<()> {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", WASM_PLUGIN_BYTES);
    environment.set_cpu_arch("aarch64");

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");
    let cache_key = plugin_cache.compute_cache_key(&plugin_source.path_source)?;
    let hash = entry_hash(&cache_key, &environment);
    let expected_file_path = wasm_artifact_path(&hash, &environment);

    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // a second request is a cache hit — no recompile
    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);
    assert!(environment.take_stderr_messages().is_empty());

    // the sidecar is written next to the artifact, no global manifest
    let meta = read_meta(&hash, &environment).unwrap();
    assert_eq!(meta.source, cache_key);
    assert_eq!(meta.plugin_kind, PluginKind::Wasm);
    assert_eq!(meta.info.name, "test-plugin");
    assert_eq!(meta.info.version, "0.2.0");
    assert_eq!(meta.local_stamps, None); // remote is content-pinned
    assert!(environment.path_exists(&expected_file_path));
    assert!(!environment.path_exists(&environment.get_cache_dir().join("plugin-cache-manifest.json")));

    // forget removes both the artifact and the sidecar
    plugin_cache.forget(&plugin_source).await.unwrap();
    assert!(!environment.path_exists(&file_path));
    assert!(read_meta(&hash, &environment).is_none());

    Ok(())
  }

  #[tokio::test]
  async fn resolve_remote_for_add_returns_checksum_and_warms_cache() -> Result<()> {
    let environment = TestEnvironment::new();
    environment.set_cpu_arch("aarch64");
    environment.add_remote_file("https://plugins.dprint.dev/test.wasm", WASM_PLUGIN_BYTES);
    let expected = crate::utils::get_sha256_checksum(WASM_PLUGIN_BYTES);
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");

    let checksum = plugin_cache.resolve_remote_for_add(&plugin_source).await?;
    assert_eq!(checksum, expected);
    // it compiled while warming the cache
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test.wasm"]);

    // the later resolve is a pure cache hit — no recompile
    let item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(item.info.name, "test-plugin");
    assert!(environment.take_stderr_messages().is_empty(), "resolve should have been a cache hit");
    Ok(())
  }

  #[tokio::test]
  async fn resolve_npm_for_add_detects_path_checksums_and_warms_cache() -> Result<()> {
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.set_cpu_arch("aarch64");
    let packument = serde_json::json!({
      "versions": { "1.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball = create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]);
    let expected = crate::utils::get_sha256_checksum(&tarball);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.0.0.tgz", tarball);
    let plugin_cache = PluginCache::new(environment.clone());
    // pathless specifier → kind detected from the package
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };

    let resolution = plugin_cache.resolve_npm_for_add(&specifier, false, None).await?;
    assert_eq!(resolution.plugin_kind, PluginKind::Wasm);
    assert_eq!(resolution.path, "plugin.wasm");
    assert_eq!(resolution.checksum, expected);
    let _ = environment.take_stderr_messages();

    // the config entry the caller would write now resolves as a cache hit
    let reference = crate::plugins::parse_plugin_source_reference(
      &format!("npm:foo@1.0.0@{}", expected),
      &PathSource::new_local(crate::environment::CanonicalizedPathBuf::new_for_testing("/dprint.json")),
      &environment,
    )?;
    let item = plugin_cache.get_plugin_cache_item(&reference).await?;
    assert_eq!(item.info.name, "test-plugin");
    assert!(environment.take_stderr_messages().is_empty(), "resolve should have been a cache hit");
    Ok(())
  }

  #[tokio::test]
  async fn should_cache_local_file() -> Result<()> {
    let environment = TestEnvironment::new();
    let original_file_path = PathBuf::from("/test.wasm");
    environment.write_file_bytes(&original_file_path, &WASM_PLUGIN_BYTES).unwrap();

    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_source = PluginSourceReference::new_local(original_file_path.clone());
    let cache_key = plugin_cache.compute_cache_key(&plugin_source.path_source)?;
    let hash = entry_hash(&cache_key, &environment);
    let expected_file_path = wasm_artifact_path(&hash, &environment);

    let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
    assert_eq!(file_path, expected_file_path);
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // cache hit on repeat; stamps were recorded for the local file
    let item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(item.file_path, expected_file_path);
    assert!(environment.take_stderr_messages().is_empty());
    let meta = read_meta(&hash, &environment).unwrap();
    let stamps = meta.local_stamps.clone().unwrap();
    assert_eq!(stamps.len(), 1);
    assert_eq!(stamps[0].path, "/test.wasm");

    // changing the file invalidates the cache and recompiles. The artifact keeps
    // the same hash-derived filename (overwritten in place — no churn).
    environment.write_file_bytes(&original_file_path, &WASM_PLUGIN_0_1_0_BYTES).unwrap();
    let item = plugin_cache.get_plugin_cache_item(&plugin_source).await?;
    assert_eq!(item.file_path, expected_file_path);
    assert_eq!(item.info.version, "0.1.0");
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // forget removes it
    plugin_cache.forget(&plugin_source).await.unwrap();
    assert!(!environment.path_exists(&file_path));
    assert!(read_meta(&hash, &environment).is_none());

    Ok(())
  }

  #[tokio::test]
  async fn local_plugin_invalidates_on_mtime_change_only() -> Result<()> {
    // identical bytes rewritten at a later time → same size, newer mtime. Proves
    // mtime alone invalidates, independent of size.
    let environment = TestEnvironment::new();
    environment.set_fs_time(1000);
    let path = PathBuf::from("/test.wasm");
    environment.write_file_bytes(&path, &WASM_PLUGIN_BYTES).unwrap();

    let plugin_cache = PluginCache::new(environment.clone());
    let source = PluginSourceReference::new_local(path.clone());
    plugin_cache.get_plugin_cache_item(&source).await?;
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // rewrite the same bytes at a newer time
    environment.set_fs_time(2000);
    environment.write_file_bytes(&path, &WASM_PLUGIN_BYTES).unwrap();
    plugin_cache.get_plugin_cache_item(&source).await?;
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling /test.wasm"]);

    // no change now → cache hit
    plugin_cache.get_plugin_cache_item(&source).await?;
    assert!(environment.take_stderr_messages().is_empty());

    Ok(())
  }

  #[tokio::test]
  async fn forget_removes_npm_extract_dir_and_artifact() -> Result<()> {
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

    // seed the npm extract dir and the compiled artifact + sidecar as if a
    // previous resolve had run
    let extract_dir = environment.get_cache_dir().join("npm").join("registry.npmjs.org").join("@dprint__test@1.0.0");
    environment.mk_dir_all(&extract_dir).unwrap();
    environment.write_file(&extract_dir.join("plugin.wasm"), "fake").unwrap();

    let cache_key = plugin_cache.compute_cache_key(&plugin_source.path_source)?;
    let hash = entry_hash(&cache_key, &environment);
    let artifact = wasm_artifact_path(&hash, &environment);
    environment.mk_dir_all(plugins_dir(&environment)).unwrap();
    environment.write_file(&artifact, "compiled").unwrap();
    write_meta(&hash, &make_wasm_meta(&cache_key, "test-plugin", "1.0.0", &environment), &environment)?;

    plugin_cache.forget(&plugin_source).await?;

    assert!(!environment.path_exists(&extract_dir));
    assert!(!environment.path_exists(&artifact));
    assert!(read_meta(&hash, &environment).is_none());
    Ok(())
  }

  #[tokio::test]
  async fn forget_unversioned_npm_removes_node_modules_entry() -> Result<()> {
    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());

    // simulate a node_modules layout so find_npm_plugin_local_path resolves
    let plugin_path = environment.cwd().join("node_modules").join("foo").join("plugin.wasm");
    environment.mk_dir_all(plugin_path.parent().unwrap()).unwrap();
    environment.write_file_bytes(&plugin_path, b"wasm").unwrap();
    let canonical = environment.canonicalize(&plugin_path).unwrap();
    let local_source = PathSource::new_local(canonical.clone());

    // unversioned npm: the resolve flow stores the entry under the local path
    // key, so forget must translate to find it
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

    let cache_key = plugin_cache.compute_cache_key(&local_source)?;
    let hash = entry_hash(&cache_key, &environment);
    environment.mk_dir_all(plugins_dir(&environment)).unwrap();
    write_meta(&hash, &make_wasm_meta(&cache_key, "foo-plugin", "1.0.0", &environment), &environment)?;
    assert!(read_meta(&hash, &environment).is_some());

    plugin_cache.forget(&plugin_source).await?;

    assert!(read_meta(&hash, &environment).is_none());
    Ok(())
  }

  #[tokio::test]
  async fn forget_unversioned_npm_with_missing_node_modules_is_noop() -> Result<()> {
    // a user deletes node_modules between dprint runs. The resolver will then
    // call forget on the original unversioned npm reference. Without a local
    // path we have no entry to remove, so forget should be a no-op instead of
    // surfacing the internal "unversioned npm" cache-key error.
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
  async fn cache_key_distinguishes_specifier_paths() -> Result<()> {
    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());
    let key = |path: &str| {
      let ps = PathSource::new_npm(
        NpmSpecifier {
          name: "foo".to_string(),
          version: Some("1.0.0".to_string()),
          path: path.to_string(),
        },
        None,
      );
      plugin_cache.compute_cache_key(&ps).unwrap()
    };
    // same name@version, different plugin file → distinct cache keys/entries
    assert_ne!(key("plugin.wasm"), key("plugin.json"));
    Ok(())
  }

  #[tokio::test]
  async fn cache_key_rejects_unversioned_npm() {
    // unversioned npm specifiers are mapped to a local path before they reach
    // the cache-key code; if that's ever skipped we want a loud error rather
    // than silently bucketing all unversioned specifiers into one entry.
    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment);
    let ps = PathSource::new_npm(
      NpmSpecifier {
        name: "foo".to_string(),
        version: None,
        path: "plugin.wasm".to_string(),
      },
      None,
    );
    let err = plugin_cache.compute_cache_key(&ps).unwrap_err();
    assert!(err.to_string().contains("unversioned npm specifier"), "got: {err}");
  }

  #[tokio::test]
  async fn cache_key_includes_registry() -> Result<()> {
    let environment = TestEnvironment::new();
    let plugin_cache = PluginCache::new(environment.clone());
    let ps = PathSource::new_npm(
      NpmSpecifier {
        name: "foo".to_string(),
        version: Some("1.0.0".to_string()),
        path: "plugin.wasm".to_string(),
      },
      None,
    );
    let default_key = plugin_cache.compute_cache_key(&ps)?;

    // a cache pointed at a private registry resolves to a different key
    environment.set_env_var("NPM_CONFIG_REGISTRY", Some("https://private.example.com"));
    let private_cache = PluginCache::new(environment.clone());
    let private_key = private_cache.compute_cache_key(&ps)?;
    assert_ne!(default_key, private_key);
    Ok(())
  }

  #[tokio::test]
  async fn local_process_plugin_stamps_include_local_archive() {
    // A process plugin whose plugin.json references a *local* archive (relative
    // path / file://) must stamp that archive too, so editing it without
    // touching plugin.json still invalidates the cache.
    let environment = TestEnvironment::new();

    let zip_bytes_v1: &[u8] = b"zip-v1";
    let zip_bytes_v2: &[u8] = b"zip-v2-different";
    let zip_checksum = get_sha256_checksum(zip_bytes_v1);
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

    let plugin_cache = PluginCache::new(environment.clone());
    let source = PathSource::new_local(environment.canonicalize(&plugin_json_path).unwrap());
    let plugin_bytes = environment.read_file_bytes(&plugin_json_path).unwrap();

    // mirrors the real call sites: capture the primary stamp, then build
    let stamps = |kind, tarball: Option<&npm_resolution::PreResolvedProcessPluginTarball>| {
      let primary = source.maybe_local_path().and_then(|p| plugin_cache.stamp_for(p));
      plugin_cache.build_local_stamps(primary, &source, &plugin_bytes, kind, tarball).unwrap()
    };

    // process plugin with a local archive → two stamps (plugin.json + bin.zip)
    let stamps_v1 = stamps(PluginKind::Process, None);
    assert_eq!(stamps_v1.len(), 2);

    // editing the archive changes the stamps — this is the regression guard
    environment.write_file_bytes(&archive_path, zip_bytes_v2).unwrap();
    let stamps_v2 = stamps(PluginKind::Process, None);
    assert_ne!(stamps_v1, stamps_v2);

    // with a pre_resolved_tarball the per-platform archive comes from npm, so
    // the local file on disk is irrelevant and is not stamped
    let dummy_tarball = npm_resolution::PreResolvedProcessPluginTarball {
      name: "p".to_string(),
      version: "0.1.0".to_string(),
      tarball_bytes: Vec::new(),
      executable_sub_path: String::new(),
    };
    assert_eq!(stamps(PluginKind::Process, Some(&dummy_tarball)).len(), 1);

    // wasm plugins only stamp the primary file
    assert_eq!(stamps(PluginKind::Wasm, None).len(), 1);
  }

  #[tokio::test]
  async fn npm_registry_resolve_caches_and_avoids_second_fetch() -> Result<()> {
    use crate::test_helpers::create_test_npm_tarball;

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
    let chained = format!("{err:#}");
    assert!(chained.contains("Resolving npm:nope"), "expected npm-spec context, got: {chained}");
    assert!(chained.contains("Could not find nope in node_modules"), "got: {chained}");
    Ok(())
  }

  #[tokio::test]
  async fn npm_node_modules_resolve_missing_plugin_file_errors() -> Result<()> {
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

    let environment = TestEnvironment::new();

    let packument = serde_json::json!({
      "versions": {
        "1.0.0": {
          "dist": { "tarball": "https://registry.npmjs.org/evil/-/evil-1.0.0.tgz" }
        }
      }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/evil", packument.to_string().into_bytes());
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
    assert!(!environment.path_exists("/etc/passwd"));
    Ok(())
  }

  #[tokio::test]
  async fn should_resolve_redirected_process_plugin_with_relative_urls() -> Result<()> {
    let environment = TestEnvironment::new();

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

    let cdn_plugin_url = "https://cdn.example.com/plugins/v1/test-process.json";
    environment.add_remote_file_bytes(cdn_plugin_url, plugin_json.as_bytes().to_vec());
    environment.add_remote_file_bytes("https://cdn.example.com/plugins/v1/test-process-plugin.zip", zip_bytes.to_vec());
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
