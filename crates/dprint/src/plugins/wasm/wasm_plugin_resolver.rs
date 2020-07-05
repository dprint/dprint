use async_trait::async_trait;
use crate::environment::Environment;
use crate::types::ErrBox;
use super::super::{Plugin, CompileFn, PluginResolver, PluginCache, PluginCacheItem};
use super::{WasmPlugin, PluginImportObject};

pub struct WasmPluginResolver<'a, TEnvironment : Environment, TCompileFn : CompileFn, TPluginImportObject : PluginImportObject> {
    environment: &'a TEnvironment,
    plugin_cache: &'a PluginCache<'a, TEnvironment, TCompileFn>,
    import_object: &'a TPluginImportObject,
}

#[async_trait(?Send)]
impl<
    'a,
    TEnvironment : Environment,
    TCompileFn : CompileFn,
    TPluginImportObject : PluginImportObject,
> PluginResolver for WasmPluginResolver<'a, TEnvironment, TCompileFn, TPluginImportObject> {
    async fn resolve_plugins(&self, urls: &Vec<String>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
        let mut plugins = Vec::new();

        for url in urls.iter() {
            let plugin = match self.resolve_plugin(url).await {
                Ok(plugin) => plugin,
                Err(err) => {
                    self.plugin_cache.forget_url(url)?;
                    return err!("Error loading plugin at url {}: {}", url, err);
                }
            };
            plugins.push(plugin);
        }

        Ok(plugins)
    }
}

impl<
    'a,
    TEnvironment : Environment,
    TCompileFn : CompileFn,
    TPluginImportObject : PluginImportObject,
> WasmPluginResolver<'a, TEnvironment, TCompileFn, TPluginImportObject> {
    pub fn new(
        environment: &'a TEnvironment,
        plugin_cache: &'a PluginCache<'a, TEnvironment, TCompileFn>,
        import_object: &'a TPluginImportObject,
    ) -> Self {
        WasmPluginResolver { environment, plugin_cache, import_object }
    }

    async fn resolve_plugin(
        &self,
        url: &str
    ) -> Result<Box<dyn Plugin>, ErrBox> {
        let import_object = self.import_object.clone();
        let cache_item = self.plugin_cache.get_plugin_cache_item(url).await;
        let cache_item: PluginCacheItem = match cache_item {
            Ok(cache_item) => Ok(cache_item),
            Err(err) => {
                self.environment.log_error(&format!(
                    "Error getting plugin from cache. Forgetting from cache and attempting redownload. Message: {:?}",
                    err
                ));

                // forget url and try again
                self.plugin_cache.forget_url(url)?;
                self.plugin_cache.get_plugin_cache_item(url).await
            }
        }?;
        let file_bytes = match self.environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
                self.environment.log_error(&format!(
                    "Error reading plugin file bytes. Forgetting from cache and attempting redownload. Message: {:?}",
                    err
                ));

                // forget url and try again
                self.plugin_cache.forget_url(url)?;
                let cache_item = self.plugin_cache.get_plugin_cache_item(url).await?;
                self.environment.read_file_bytes(&cache_item.file_path)?
            }
        };

        Ok(Box::new(WasmPlugin::new(file_bytes, cache_item.info, import_object)))
    }
}
