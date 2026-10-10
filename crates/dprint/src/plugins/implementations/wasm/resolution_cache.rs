use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::PathBuf;

use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::plugins::FileMatchingInfo;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::utils::FastInsecureHasher;

/// Number of configurations remembered per plugin before the oldest is dropped.
const MAX_ENTRIES: usize = 10;

/// Caches what a Wasm plugin derives from a configuration (its file matching
/// info, resolved configuration and configuration diagnostics) in a sidecar
/// file beside the plugin's compiled module.
///
/// Wasm plugins are sandboxed, so these values are a pure function of the
/// plugin and the configuration. Serving them from here lets commands that
/// never format a file (ex. `output-file-paths`, or an incremental run where
/// every file is already formatted) skip creating a Wasm instance entirely.
pub struct WasmPluginResolutionCache<TEnvironment: Environment> {
  file_path: PathBuf,
  environment: TEnvironment,
  /// Loaded from disk on first use.
  file: Mutex<Option<CacheFile>>,
}

impl<TEnvironment: Environment> WasmPluginResolutionCache<TEnvironment> {
  pub fn new(file_path: PathBuf, environment: TEnvironment) -> Self {
    Self {
      file_path,
      environment,
      file: Default::default(),
    }
  }

  pub fn get_file_matching_info(&self, config: &FormatConfig) -> Option<FileMatchingInfo> {
    self.with_entry(config, |entry| entry.file_matching_info.clone())
  }

  pub fn set_file_matching_info(&self, config: &FormatConfig, value: &FileMatchingInfo) {
    self.update_entry(config, |entry| entry.file_matching_info = Some(value.clone()));
  }

  pub fn get_resolved_config(&self, config: &FormatConfig) -> Option<String> {
    self.with_entry(config, |entry| entry.resolved_config.clone())
  }

  pub fn set_resolved_config(&self, config: &FormatConfig, value: &str) {
    self.update_entry(config, |entry| entry.resolved_config = Some(value.to_string()));
  }

  pub fn get_config_diagnostics(&self, config: &FormatConfig) -> Option<Vec<ConfigurationDiagnostic>> {
    self.with_entry(config, |entry| entry.config_diagnostics.clone())
  }

  pub fn set_config_diagnostics(&self, config: &FormatConfig, value: &[ConfigurationDiagnostic]) {
    self.update_entry(config, |entry| entry.config_diagnostics = Some(value.to_vec()));
  }

  fn with_entry<T>(&self, config: &FormatConfig, get_value: impl FnOnce(&CacheEntry) -> Option<T>) -> Option<T> {
    let config_hash = config_hash(config);
    let mut file = self.file.lock();
    let file = file.get_or_insert_with(|| self.read_file());
    file.entries.iter().find(|entry| entry.config_hash == config_hash).and_then(get_value)
  }

  fn update_entry(&self, config: &FormatConfig, update: impl FnOnce(&mut CacheEntry)) {
    let config_hash = config_hash(config);
    let mut file = self.file.lock();
    let file = file.get_or_insert_with(|| self.read_file());
    let entry = match file.entries.iter().position(|entry| entry.config_hash == config_hash) {
      Some(index) => &mut file.entries[index],
      None => {
        if file.entries.len() >= MAX_ENTRIES {
          file.entries.remove(0);
        }
        file.entries.push(CacheEntry {
          config_hash,
          file_matching_info: None,
          resolved_config: None,
          config_diagnostics: None,
        });
        file.entries.last_mut().unwrap()
      }
    };
    update(entry);
    self.write_file(file);
  }

  fn read_file(&self) -> CacheFile {
    let cli_version = self.environment.cli_version();
    let maybe_file = self
      .environment
      .read_file(&self.file_path)
      .ok()
      .and_then(|text| serde_json::from_str::<CacheFile>(&text).ok())
      // the serialized shape of the cached values may change between versions
      .filter(|file| file.cli_version == cli_version);
    maybe_file.unwrap_or_else(|| CacheFile {
      cli_version,
      entries: Vec::new(),
    })
  }

  fn write_file(&self, file: &CacheFile) {
    // this is only a cache, so a failure to write it is not an error
    let result = serde_json::to_vec(file)
      .map_err(anyhow::Error::from)
      .and_then(|bytes| Ok(self.environment.atomic_write_file_bytes(&self.file_path, &bytes)?));
    if let Err(err) = result {
      log_debug!(self.environment, "Failed writing {}. {:#}", self.file_path.display(), err);
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile {
  cli_version: String,
  entries: Vec<CacheEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
  config_hash: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  file_matching_info: Option<FileMatchingInfo>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  resolved_config: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  config_diagnostics: Option<Vec<ConfigurationDiagnostic>>,
}

/// Hashes the parts of the configuration the plugin sees. The config id is
/// left out because it's only meaningful within the current process.
fn config_hash(config: &FormatConfig) -> String {
  let mut hasher = FastInsecureHasher::default();
  // sort the keys so the hash doesn't depend on the order they were specified in
  let sorted_plugin_config = config.plugin.iter().collect::<BTreeMap<_, _>>();
  hasher.write(&serde_json::to_vec(&sorted_plugin_config).unwrap_or_default());
  hasher.write(&[0]);
  hasher.write(&serde_json::to_vec(&config.global).unwrap_or_default());
  format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod test {
  use dprint_core::configuration::ConfigKeyMap;
  use dprint_core::configuration::ConfigKeyValue;
  use dprint_core::configuration::GlobalConfiguration;
  use dprint_core::plugins::FormatConfigId;

  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn should_cache_values_per_config() {
    let environment = TestEnvironment::new();
    let cache = WasmPluginResolutionCache::new(PathBuf::from("/cache/plugin.resolved.json"), environment.clone());
    let config1 = config(1, &[("lineWidth", 80)]);
    let config2 = config(2, &[("lineWidth", 120)]);

    assert_eq!(cache.get_resolved_config(&config1), None);
    cache.set_resolved_config(&config1, "{\"lineWidth\":80}");
    cache.set_file_matching_info(&config1, &file_matching_info(&["ts"]));
    cache.set_config_diagnostics(&config1, &[]);

    assert_eq!(cache.get_resolved_config(&config1).as_deref(), Some("{\"lineWidth\":80}"));
    assert_eq!(cache.get_file_matching_info(&config1), Some(file_matching_info(&["ts"])));
    assert_eq!(cache.get_config_diagnostics(&config1), Some(Vec::new()));
    assert_eq!(cache.get_resolved_config(&config2), None);
    assert_eq!(cache.get_file_matching_info(&config2), None);
    assert_eq!(cache.get_config_diagnostics(&config2), None);

    // a new cache reading the same file should see the values
    let cache = WasmPluginResolutionCache::new(PathBuf::from("/cache/plugin.resolved.json"), environment);
    assert_eq!(cache.get_resolved_config(&config1).as_deref(), Some("{\"lineWidth\":80}"));
    assert_eq!(cache.get_file_matching_info(&config1), Some(file_matching_info(&["ts"])));
  }

  #[test]
  fn should_ignore_config_id_and_key_order() {
    let config1 = FormatConfig {
      id: FormatConfigId::from_raw(1),
      plugin: ConfigKeyMap::from([("a".to_string(), ConfigKeyValue::Number(1)), ("b".to_string(), ConfigKeyValue::Number(2))]),
      global: Default::default(),
    };
    let config2 = FormatConfig {
      id: FormatConfigId::from_raw(2),
      plugin: ConfigKeyMap::from([("b".to_string(), ConfigKeyValue::Number(2)), ("a".to_string(), ConfigKeyValue::Number(1))]),
      global: Default::default(),
    };
    assert_eq!(config_hash(&config1), config_hash(&config2));

    let config3 = FormatConfig {
      global: GlobalConfiguration {
        line_width: Some(80),
        ..Default::default()
      },
      ..config1
    };
    assert_ne!(config_hash(&config2), config_hash(&config3));
  }

  #[test]
  fn should_ignore_file_from_other_cli_version_or_corrupt() {
    let environment = TestEnvironment::new();
    let file_path = PathBuf::from("/cache/plugin.resolved.json");
    let config = config(1, &[]);

    let cache = WasmPluginResolutionCache::new(file_path.clone(), environment.clone());
    cache.set_resolved_config(&config, "{}");
    let text = environment.read_file(&file_path).unwrap();
    environment
      .write_file(&file_path, &text.replace("\"cliVersion\":\"0.0.0\"", "\"cliVersion\":\"0.0.1\""))
      .unwrap();
    let cache = WasmPluginResolutionCache::new(file_path.clone(), environment.clone());
    assert_eq!(cache.get_resolved_config(&config), None);

    environment.write_file(&file_path, "{ not json").unwrap();
    let cache = WasmPluginResolutionCache::new(file_path.clone(), environment.clone());
    assert_eq!(cache.get_resolved_config(&config), None);
    // writing should recover the file
    cache.set_resolved_config(&config, "{}");
    let cache = WasmPluginResolutionCache::new(file_path, environment);
    assert_eq!(cache.get_resolved_config(&config).as_deref(), Some("{}"));
  }

  #[test]
  fn should_drop_oldest_entries() {
    let environment = TestEnvironment::new();
    let cache = WasmPluginResolutionCache::new(PathBuf::from("/cache/plugin.resolved.json"), environment);
    for i in 0..=MAX_ENTRIES {
      cache.set_resolved_config(&config(i as u32, &[("value", i as i32)]), &i.to_string());
    }
    assert_eq!(cache.get_resolved_config(&config(0, &[("value", 0)])), None);
    assert_eq!(cache.get_resolved_config(&config(1, &[("value", 1)])).as_deref(), Some("1"));
    assert_eq!(
      cache
        .get_resolved_config(&config(MAX_ENTRIES as u32, &[("value", MAX_ENTRIES as i32)]))
        .as_deref(),
      Some(MAX_ENTRIES.to_string().as_str())
    );
  }

  fn config(id: u32, plugin: &[(&str, i32)]) -> FormatConfig {
    FormatConfig {
      id: FormatConfigId::from_raw(id),
      plugin: plugin.iter().map(|(key, value)| (key.to_string(), ConfigKeyValue::Number(*value))).collect(),
      global: Default::default(),
    }
  }

  fn file_matching_info(extensions: &[&str]) -> FileMatchingInfo {
    FileMatchingInfo {
      file_extensions: extensions.iter().map(|ext| ext.to_string()).collect(),
      file_names: Vec::new(),
      additive: false,
    }
  }
}
