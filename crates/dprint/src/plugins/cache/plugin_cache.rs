use std::path::PathBuf;
use dprint_core::plugins::PluginInfo;

use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::CompileFn;

pub struct PluginCache<'a, TEnvironment : Environment, TCompileFn: CompileFn> {
    environment: &'a TEnvironment,
    cache: &'a Cache<'a, TEnvironment>,
    compile: &'a TCompileFn,
}

pub struct PluginCacheItem {
    pub file_path: PathBuf,
    pub info: PluginInfo,
}

impl<'a, TEnvironment, TCompileFn> PluginCache<'a, TEnvironment, TCompileFn> where TEnvironment : Environment, TCompileFn : CompileFn {
    pub fn new(environment: &'a TEnvironment, cache: &'a Cache<'a, TEnvironment>, compile: &'a TCompileFn) -> Self {
        PluginCache {
            environment,
            cache,
            compile,
        }
    }

    pub fn forget_url(&self, url: &str) -> Result<(), ErrBox> {
        self.cache.forget_item(&self.get_cache_key(url))
    }

    pub async fn get_plugin_cache_item(&self, url: &str) -> Result<PluginCacheItem, ErrBox> {
        let cache_key = self.get_cache_key(url);
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

            return Ok(PluginCacheItem {
                file_path,
                info: plugin_info,
            });
        }

        let file_bytes = self.environment.download_file(url).await?;
        self.environment.log("Compiling wasm module...");
        let compile_result = (self.compile)(&file_bytes)?;
        let serialized_plugin_info = match serde_json::to_string(&compile_result.plugin_info) {
            Ok(serialized_plugin_info) => serialized_plugin_info,
            Err(err) => return err!("Error serializing plugin info. {:?}", err),
        };

        let cache_item = self.cache.create_cache_item(CreateCacheItemOptions {
            key: cache_key,
            extension: "compiled_wasm",
            bytes: &compile_result.bytes,
            meta_data: Some(serialized_plugin_info),
        })?;
        let file_path = self.cache.resolve_cache_item_file_path(&cache_item);

        Ok(PluginCacheItem {
            file_path,
            info: compile_result.plugin_info,
        })
    }

    fn get_cache_key(&self, url: &str) -> String {
        // add a prefix
        format!("plugin:{}", url)
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use dprint_core::plugins::PluginInfo;
    use crate::environment::TestEnvironment;
    use crate::plugins::CompilationResult;
    use crate::types::ErrBox;
    use super::*;

    #[tokio::test]
    async fn it_should_download_file() -> Result<(), ErrBox> {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://plugins.dprint.dev/test.wasm", "t".as_bytes());

        let cache = Cache::new(&environment).unwrap();
        let plugin_cache = PluginCache::new(&environment, &cache, &identity_compile);
        let file_path = plugin_cache.get_plugin_cache_item("https://plugins.dprint.dev/test.wasm").await?.file_path;
        let expected_file_path = PathBuf::from("/cache").join("test.compiled_wasm");

        assert_eq!(file_path, expected_file_path);

        // should be the same when requesting it again
        let file_path = plugin_cache.get_plugin_cache_item("https://plugins.dprint.dev/test.wasm").await?.file_path;
        assert_eq!(file_path, expected_file_path);

        // should have saved the manifest
        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{"plugin:https://plugins.dprint.dev/test.wasm":{"fileName":"test.compiled_wasm","createdTime":123456,"metaData":"{\"name\":\"test-plugin\",\"version\":\"0.1.0\",\"configKey\":\"test-plugin\",\"fileExtensions\":[\"txt\",\"dat\"],\"helpUrl\":\"test-url\",\"configSchemaUrl\":\"schema-url\"}"}}"#,
        );
        Ok(())
    }

    #[tokio::test]
    async fn it_should_forget_a_url() -> Result<(), ErrBox> {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://plugins.dprint.dev/test.wasm", "t".as_bytes());

        let cache = Cache::new(&environment).unwrap();
        let plugin_cache = PluginCache::new(&environment, &cache, &identity_compile);
        let file_path = plugin_cache.get_plugin_cache_item("https://plugins.dprint.dev/test.wasm").await?.file_path;
        assert_eq!(environment.path_exists(&file_path), true);

        // should forget it afterwards
        plugin_cache.forget_url("https://plugins.dprint.dev/test.wasm").unwrap();

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
