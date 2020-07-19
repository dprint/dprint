use std::path::PathBuf;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use dprint_core::plugins::PluginInfo;

use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::{CompileFn, PluginSourceReference};
use crate::utils::{PathSource, RemotePathSource, LocalPathSource, get_bytes_hash, verify_sha256_checksum};

#[derive(Clone)]
pub struct WasmPluginCache<TEnvironment : Environment, TCompileFn: CompileFn> {
    environment: TEnvironment,
    cache: Arc<Cache<TEnvironment>>,
    compile: &'static TCompileFn,
}

pub struct WasmPluginCacheItem {
    pub file_path: PathBuf,
    pub info: PluginInfo,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LocalPluginMetaData {
    file_hash: u64,
    plugin_info: PluginInfo,
}

impl<TEnvironment, TCompileFn> WasmPluginCache<TEnvironment, TCompileFn> where TEnvironment : Environment, TCompileFn : CompileFn {
    pub fn new(environment: TEnvironment, cache: Arc<Cache<TEnvironment>>, compile: &'static TCompileFn) -> Self {
        WasmPluginCache {
            environment,
            cache,
            compile,
        }
    }

    pub fn forget(&self, source_reference: &PluginSourceReference) -> Result<(), ErrBox> {
        self.cache.forget_item(&self.get_cache_key(&source_reference.path_source)?)
    }

    pub async fn get_plugin_cache_item(&self, source_reference: &PluginSourceReference) -> Result<WasmPluginCacheItem, ErrBox> {
        let cache_key = self.get_cache_key(&source_reference.path_source)?;
        match &source_reference.path_source {
            PathSource::Remote(remote_source) => self.get_remote_plugin(cache_key, remote_source, &source_reference.checksum).await,
            PathSource::Local(local_source) => self.get_local_plugin(cache_key, local_source, &source_reference.checksum).await,
        }
    }

    async fn get_remote_plugin(&self, cache_key: String, remote_source: &RemotePathSource, checksum: &Option<String>) -> Result<WasmPluginCacheItem, ErrBox> {
        if let Some(cache_item) = self.cache.get_cache_item(&cache_key) {
            let file_path = self.cache.resolve_cache_item_file_path(&cache_item);
            let meta_data = match &cache_item.meta_data {
                Some(meta_data) => meta_data,
                None => return err!("Expected to have plugin info stored in the cache."),
            };
            let plugin_info = match serde_json::from_str(meta_data) {
                Ok(plugin_info) => plugin_info,
                Err(err) => return err!("Error deserializing plugin info. {:?}", err),
            };

            return Ok(WasmPluginCacheItem {
                file_path,
                info: plugin_info,
            });
        }

        // get bytes
        let url_str = remote_source.url.as_str();
        let file_bytes = self.environment.download_file(url_str).await?;

        // check checksum only if provided (not required for WASM plugins)
        if let Some(checksum) = &checksum {
            verify_sha256_checksum(&file_bytes, checksum)?;
        }

        // compile
        let compile_result = self.environment.log_action_with_progress(&format!("Compiling {}", url_str), || {
            (self.compile)(&file_bytes)
        }).await??;

        let serialized_plugin_info = match serde_json::to_string(&compile_result.plugin_info) {
            Ok(serialized_plugin_info) => serialized_plugin_info,
            Err(err) => return err!("Error serializing plugin info. {:?}", err),
        };

        let cache_item = self.cache.create_cache_item(CreateCacheItemOptions {
            key: cache_key,
            extension: "compiled_wasm",
            bytes: Some(&compile_result.bytes),
            meta_data: Some(serialized_plugin_info),
        })?;
        let file_path = self.cache.resolve_cache_item_file_path(&cache_item);

        Ok(WasmPluginCacheItem {
            file_path,
            info: compile_result.plugin_info,
        })
    }

    async fn get_local_plugin(&self, cache_key: String, local_source: &LocalPathSource, checksum: &Option<String>) -> Result<WasmPluginCacheItem, ErrBox> {
        let file_bytes = self.environment.read_file_bytes(&local_source.path)?;
        let file_hash = get_bytes_hash(&file_bytes);
        if let Some(cache_item) = self.cache.get_cache_item(&cache_key) {
            let file_path = self.cache.resolve_cache_item_file_path(&cache_item);
            let serialized_meta_data = match &cache_item.meta_data {
                Some(meta_data) => meta_data,
                None => return err!("Expected to have plugin info stored in the cache."),
            };
            let meta_data: LocalPluginMetaData = match serde_json::from_str(serialized_meta_data) {
                Ok(meta_data) => meta_data,
                Err(err) => return err!("Error deserializing plugin info. {:?}", err),
            };

            if meta_data.file_hash == file_hash {
                return Ok(WasmPluginCacheItem {
                    file_path,
                    info: meta_data.plugin_info,
                });
            } else {
                self.cache.forget_item(&cache_key)?;
            }
        }

        if let Some(checksum) = &checksum {
            verify_sha256_checksum(&file_bytes, checksum)?;
        }

        let compile_result = self.environment.log_action_with_progress("Compiling wasm module...", || {
            (self.compile)(&file_bytes)
        }).await??;
        let meta_data = LocalPluginMetaData {
            plugin_info: compile_result.plugin_info.clone(),
            file_hash,
        };
        let serialized_meta_data = match serde_json::to_string(&meta_data) {
            Ok(serialized_plugin_info) => serialized_plugin_info,
            Err(err) => return err!("Error serializing plugin info. {:?}", err),
        };

        let cache_item = self.cache.create_cache_item(CreateCacheItemOptions {
            key: cache_key,
            extension: "compiled_wasm",
            bytes: Some(&compile_result.bytes),
            meta_data: Some(serialized_meta_data),
        })?;
        let file_path = self.cache.resolve_cache_item_file_path(&cache_item);

        Ok(WasmPluginCacheItem {
            file_path,
            info: compile_result.plugin_info,
        })
    }

    fn get_cache_key(&self, path_source: &PathSource) -> Result<String, ErrBox> {
        // add a prefix
        let mut key = String::from("plugin:");
        match path_source {
            PathSource::Remote(remote_source) => key.push_str(remote_source.url.as_str()),
            PathSource::Local(local_source) => {
                let absolute_path = self.environment.canonicalize(&local_source.path)?;
                key.push_str(&absolute_path.to_string_lossy());
            }
        }
        Ok(key)
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use dprint_core::plugins::PluginInfo;
    use crate::environment::TestEnvironment;
    use crate::plugins::{CompilationResult, PluginSourceReference};
    use crate::types::ErrBox;
    use super::*;

    #[tokio::test]
    async fn it_should_download_remote_file() -> Result<(), ErrBox> {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://plugins.dprint.dev/test.wasm", "t".as_bytes());

        let cache = Arc::new(Cache::new(environment.clone()).unwrap());
        let plugin_cache = WasmPluginCache::new(environment.clone(), cache, &identity_compile);
        let plugin_source = PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test.wasm");
        let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
        let expected_file_path = PathBuf::from("/cache").join("test.compiled_wasm");

        assert_eq!(file_path, expected_file_path);

        // should be the same when requesting it again
        let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
        assert_eq!(file_path, expected_file_path);

        // should have saved the manifest
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{"plugin:https://plugins.dprint.dev/test.wasm":{"fileName":"test.compiled_wasm","createdTime":123456,"metaData":"{\"name\":\"test-plugin\",\"version\":\"0.1.0\",\"configKey\":\"test-plugin\",\"fileExtensions\":[\"txt\",\"dat\"],\"helpUrl\":\"test-url\",\"configSchemaUrl\":\"schema-url\"}"}}"#,
        );

        // should forget it afterwards
        plugin_cache.forget(&plugin_source).unwrap();

        assert_eq!(environment.path_exists(&file_path), false);
        // should have saved the manifest
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{}"#,
        );

        Ok(())
    }

    #[tokio::test]
    async fn it_should_cache_local_file() -> Result<(), ErrBox> {
        let environment = TestEnvironment::new();
        let original_file_path = PathBuf::from("/test.wasm");
        let file_bytes = "t".as_bytes();
        environment.write_file_bytes(&original_file_path, file_bytes).unwrap();

        let cache = Arc::new(Cache::new(environment.clone()).unwrap());
        let plugin_cache = WasmPluginCache::new(environment.clone(), cache, &identity_compile);
        let plugin_source = PluginSourceReference::new_local(original_file_path.clone());
        let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
        let expected_file_path = PathBuf::from("/cache").join("test.compiled_wasm");

        assert_eq!(file_path, expected_file_path);

        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec!["Compiling wasm module..."]);
        environment.clear_logs();

        // should be the same when requesting it again
        let file_path = plugin_cache.get_plugin_cache_item(&plugin_source).await?.file_path;
        assert_eq!(file_path, expected_file_path);

        // should have saved the manifest
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            concat!(
                r#"{"plugin:/test.wasm":{"fileName":"test.compiled_wasm","createdTime":123456,"metaData":"{"#,
                r#"\"fileHash\":10632242795325663332,\"pluginInfo\":{"#,
                r#"\"name\":\"test-plugin\",\"version\":\"0.1.0\",\"configKey\":\"test-plugin\","#,
                r#"\"fileExtensions\":[\"txt\",\"dat\"],\"helpUrl\":\"test-url\",\"configSchemaUrl\":\"schema-url\"}}"}}"#,
            )
        );

        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages.len(), 0); // no logs, nothing changed
        environment.clear_logs();

        // update the file bytes
        let file_bytes = "u".as_bytes();
        environment.write_file_bytes(&original_file_path, file_bytes).unwrap();

        // should update the cache with the new file
        let file_path = plugin_cache.get_plugin_cache_item(&PluginSourceReference::new_local(original_file_path.clone())).await?.file_path;
        assert_eq!(file_path, expected_file_path);

        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            concat!(
                r#"{"plugin:/test.wasm":{"fileName":"test.compiled_wasm","createdTime":123456,"metaData":"{"#,
                r#"\"fileHash\":6989588595861227504,\"pluginInfo\":{"#,
                r#"\"name\":\"test-plugin\",\"version\":\"0.1.0\",\"configKey\":\"test-plugin\","#,
                r#"\"fileExtensions\":[\"txt\",\"dat\"],\"helpUrl\":\"test-url\",\"configSchemaUrl\":\"schema-url\"}}"}}"#,
            )
        );

        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec!["Compiling wasm module..."]);
        environment.clear_logs();

        // should forget it afterwards
        plugin_cache.forget(&plugin_source).unwrap();

        assert_eq!(environment.path_exists(&file_path), false);
        // should have saved the manifest
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{}"#,
        );

        Ok(())
    }

    fn identity_compile(bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
        Ok(CompilationResult {
            bytes: bytes.to_vec(),
            plugin_info: get_test_plugin_info(),
        })
    }

    fn get_test_plugin_info() -> PluginInfo {
        PluginInfo {
            name: String::from("test-plugin"),
            version: String::from("0.1.0"),
            config_key: String::from("test-plugin"),
            file_extensions: vec![String::from("txt"), String::from("dat")],
            help_url: String::from("test-url"),
            config_schema_url: String::from("schema-url"),
        }
    }
}
