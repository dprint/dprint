use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use dprint_core::plugins::PluginInfo;

use super::implementations::WASM_CACHE_VERSION;
use crate::environment::Environment;
use crate::utils::FastInsecureHasher;
use crate::utils::PluginKind;
use std::hash::Hasher;

/// Bumped when the on-disk cache layout or meta format changes in a way that
/// should invalidate existing entries. Folded into each entry's signature so a
/// bump simply orphans old entries (they stay on disk until `clear-cache`)
/// rather than busting the whole cache.
const PLUGIN_CACHE_SCHEMA_VERSION: usize = 10;

/// Size + modification time of a local file, captured at setup. A cache hit
/// requires every stamp to still match (cheap stat, no read/hash).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalStamp {
  pub path: String,
  pub len: u64,
  pub modified_ms: u64,
}

/// Sidecar describing a single cached plugin. Lives next to its artifact at
/// `plugins/<hash>.json`, where `<hash>` is derived from the plugin's source
/// (see [`entry_hash`]). Replaces the old global `plugin-cache-manifest.json`
/// so changing one plugin never rewrites state for the others.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheMeta {
  /// The cache key this entry was stored under (e.g. `remote:<url>`). Compared
  /// against the looked-up key to reject the astronomically unlikely event of
  /// two distinct sources hashing to the same filename.
  pub source: String,
  /// Identifies the toolchain/arch the artifact was built for. Correctness is
  /// already enforced by folding this into the entry's hash (a change produces a
  /// fresh filename); this stored copy isn't read today, but is retained so a
  /// future cleanup pass could identify entries left by an old toolchain, and to
  /// keep the sidecar self-describing.
  pub signature: String,
  pub plugin_kind: PluginKind,
  /// Created time in *seconds* since epoch. Recorded for human inspection of the
  /// sidecar only — nothing reads it today (kept in case a future cleanup pass
  /// wants an age signal).
  pub created_time: u64,
  pub info: PluginInfo,
  /// Executable path relative to the plugin's extract dir. Process plugins only.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub executable_sub_path: Option<String>,
  /// Modification stamps for the local source file(s). Present only for local
  /// sources, where edits must invalidate the cache; absent for content-pinned
  /// remote and versioned-npm sources, whose mere presence is a cache hit.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub local_stamps: Option<Vec<LocalStamp>>,
}

impl PluginCacheMeta {
  /// The on-disk file path of this entry's artifact: the compiled module for
  /// wasm plugins, or the executable within the extract dir for process plugins.
  pub fn artifact_file_path(&self, hash: &str, environment: &impl Environment) -> PathBuf {
    match self.plugin_kind {
      PluginKind::Wasm => wasm_artifact_path(hash, environment),
      PluginKind::Process => {
        let sub_path = self.executable_sub_path.as_deref().unwrap_or_default();
        process_dir_path(hash, environment).join(sub_path)
      }
    }
  }
}

/// The signature folded into every cache key + stored in each entry. A change
/// here means existing artifacts are no longer valid for this machine/build, so
/// they get a fresh hash; the now-unreferenced old files stay on disk until
/// `clear-cache`.
pub fn current_signature(environment: &impl Environment) -> String {
  // `wasm_cache_key` already covers cpu arch + rustc version; `os` distinguishes
  // musl/glibc/etc for process plugins. Process plugins technically don't care
  // about the wasm bits, but folding everything in keeps the key kind-agnostic
  // (so we can hash before resolving an npm plugin's kind) at the only cost of a
  // rare, cheap re-extract on a dprint upgrade.
  format!(
    "{}-{}-{}-{}",
    PLUGIN_CACHE_SCHEMA_VERSION,
    WASM_CACHE_VERSION,
    environment.wasm_cache_key(),
    environment.os(),
  )
}

/// Hashes a plugin's cache key (e.g. `remote:<url>`) together with the current
/// signature into the stable, opaque filename stem used for its sidecar and
/// artifact. Folding the signature in means different arches/toolchains get
/// distinct files and can coexist in a shared cache dir.
pub fn entry_hash(cache_key: &str, environment: &impl Environment) -> String {
  let mut hasher = FastInsecureHasher::default();
  hasher.write(current_signature(environment).as_bytes());
  hasher.write(&[0]);
  hasher.write(cache_key.as_bytes());
  format!("{:016x}", hasher.finish())
}

/// Reads the sidecar for `hash`. Returns `None` if it's missing or unparseable
/// (treated as a cache miss — the caller re-sets-up and overwrites).
pub fn read_meta(hash: &str, environment: &impl Environment) -> Option<PluginCacheMeta> {
  let text = environment.read_file(meta_path(hash, environment)).ok()?;
  serde_json::from_str(&text).ok()
}

pub fn write_meta(hash: &str, meta: &PluginCacheMeta, environment: &impl Environment) -> Result<()> {
  let serialized = serde_json::to_string(meta)?;
  Ok(environment.atomic_write_file_bytes(meta_path(hash, environment), serialized.as_bytes())?)
}

/// Removes an entry's sidecar and artifact(s). Kind-agnostic: deletes the meta
/// json, the wasm artifact, and the process extract dir, ignoring whichever
/// don't exist.
pub fn remove_entry(hash: &str, environment: &impl Environment) {
  let _ = environment.remove_file(meta_path(hash, environment));
  let _ = environment.remove_file(wasm_artifact_path(hash, environment));
  environment.try_remove_dir_all(process_dir_path(hash, environment));
}

/// Converts a modification time to milliseconds since the unix epoch for stable
/// serialization. Saturates to 0 for the (pre-epoch) edge case.
pub fn to_unix_millis(time: SystemTime) -> u64 {
  time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn plugins_dir(environment: &impl Environment) -> PathBuf {
  environment.get_cache_dir().join("plugins")
}

/// Destination for a wasm plugin's compiled artifact.
pub fn wasm_artifact_path(hash: &str, environment: &impl Environment) -> PathBuf {
  plugins_dir(environment).join(format!("{hash}.cwasm"))
}

/// Destination directory a process plugin is extracted into.
pub fn process_dir_path(hash: &str, environment: &impl Environment) -> PathBuf {
  plugins_dir(environment).join(hash)
}

fn meta_path(hash: &str, environment: &impl Environment) -> PathBuf {
  plugins_dir(environment).join(format!("{hash}.json"))
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use pretty_assertions::assert_eq;

  fn make_meta(signature: &str) -> PluginCacheMeta {
    PluginCacheMeta {
      source: "remote:https://example.com/test.wasm".to_string(),
      signature: signature.to_string(),
      plugin_kind: PluginKind::Wasm,
      created_time: 123,
      info: PluginInfo {
        name: "test-plugin".to_string(),
        version: "0.1.0".to_string(),
        config_key: "test".to_string(),
        help_url: "help".to_string(),
        config_schema_url: "schema".to_string(),
        update_url: None,
      },
      executable_sub_path: None,
      local_stamps: None,
    }
  }

  #[test]
  fn should_roundtrip_meta() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all(plugins_dir(&environment)).unwrap();
    let meta = make_meta(&current_signature(&environment));
    write_meta("abc123", &meta, &environment).unwrap();
    assert_eq!(read_meta("abc123", &environment), Some(meta));
  }

  #[test]
  fn read_meta_is_none_for_missing_or_corrupt() {
    let environment = TestEnvironment::new();
    assert_eq!(read_meta("missing", &environment), None);
    environment.mk_dir_all(plugins_dir(&environment)).unwrap();
    environment.write_file(&plugins_dir(&environment).join("bad.json"), "{ not json").unwrap();
    assert_eq!(read_meta("bad", &environment), None);
  }

  #[test]
  fn remove_entry_deletes_meta_and_both_artifact_forms() {
    let environment = TestEnvironment::new();
    let dir = plugins_dir(&environment);
    environment.mk_dir_all(&dir).unwrap();
    write_meta("h", &make_meta("sig"), &environment).unwrap();
    environment.write_file(&wasm_artifact_path("h", &environment), "compiled").unwrap();
    environment.mk_dir_all(process_dir_path("h", &environment)).unwrap();
    environment.write_file(&process_dir_path("h", &environment).join("exe"), "bin").unwrap();

    remove_entry("h", &environment);

    assert!(read_meta("h", &environment).is_none());
    assert!(!environment.path_exists(&wasm_artifact_path("h", &environment)));
    assert!(!environment.path_exists(&process_dir_path("h", &environment)));
  }
}
