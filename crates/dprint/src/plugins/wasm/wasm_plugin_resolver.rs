use async_trait::async_trait;
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::utils::PathSource;
use super::super::{Plugin, CompileFn, PluginResolver, PluginCache, PluginCacheItem};
use super::{WasmPlugin, ImportObjectFactory};

pub struct WasmPluginResolver<'a, TEnvironment : Environment, TCompileFn : CompileFn, TImportObjectFactory : ImportObjectFactory> {
    environment: &'a TEnvironment,
    plugin_cache: &'a PluginCache<'a, TEnvironment, TCompileFn>,
    import_object_factory: &'a TImportObjectFactory,
}

#[async_trait(?Send)]
impl<
    'a,
    TEnvironment : Environment,
    TCompileFn : CompileFn,
    TImportObjectFactory : ImportObjectFactory,
> PluginResolver for WasmPluginResolver<'a, TEnvironment, TCompileFn, TImportObjectFactory> {
    async fn resolve_plugins(&self, path_sources: &Vec<PathSource>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
        let mut plugins = Vec::new();

        for path_source in path_sources.iter() {
            let plugin = match self.resolve_plugin(path_source).await {
                Ok(plugin) => plugin,
                Err(err) => {
                    self.plugin_cache.forget(path_source)?;
                    return err!("Error resolving plugin {}: {}", path_source.display(), err);
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
    TImportObjectFactory : ImportObjectFactory,
> WasmPluginResolver<'a, TEnvironment, TCompileFn, TImportObjectFactory> {
    pub fn new(
        environment: &'a TEnvironment,
        plugin_cache: &'a PluginCache<'a, TEnvironment, TCompileFn>,
        import_object_factory: &'a TImportObjectFactory,
    ) -> Self {
        WasmPluginResolver { environment, plugin_cache, import_object_factory }
    }

    async fn resolve_plugin(
        &self,
        path_source: &PathSource,
    ) -> Result<Box<dyn Plugin>, ErrBox> {
        let import_object_factory = self.import_object_factory.clone();
        let cache_item = self.plugin_cache.get_plugin_cache_item(path_source).await;
        let cache_item: PluginCacheItem = match cache_item {
            Ok(cache_item) => Ok(cache_item),
            Err(err) => {
                self.environment.log_error(&format!(
                    "Error getting plugin from cache. Forgetting from cache and retrying. Message: {:?}",
                    err
                ));

                // forget and try again
                self.plugin_cache.forget(path_source)?;
                self.plugin_cache.get_plugin_cache_item(path_source).await
            }
        }?;
        let file_bytes = match self.environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
                self.environment.log_error(&format!(
                    "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {:?}",
                    err
                ));

                // forget and try again
                self.plugin_cache.forget(path_source)?;
                let cache_item = self.plugin_cache.get_plugin_cache_item(path_source).await?;
                self.environment.read_file_bytes(&cache_item.file_path)?
            }
        };

        Ok(Box::new(WasmPlugin::new(file_bytes, cache_item.info, import_object_factory)))
    }
}
