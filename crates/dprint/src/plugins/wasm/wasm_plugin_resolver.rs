use async_trait::async_trait;

use crate::environment::Environment;
use crate::types::ErrBox;
use crate::utils::PathSource;
use super::super::{Plugin, CompileFn, PluginResolver, PluginCache, PluginCacheItem};
use super::{WasmPlugin, ImportObjectFactory};

pub struct WasmPluginResolver<TEnvironment : Environment, TCompileFn : CompileFn, TImportObjectFactory : ImportObjectFactory> {
    environment: TEnvironment,
    plugin_cache: PluginCache<TEnvironment, TCompileFn>,
    import_object_factory: TImportObjectFactory,
}

#[async_trait(?Send)]
impl<
    TEnvironment : Environment,
    TCompileFn : CompileFn,
    TImportObjectFactory : ImportObjectFactory,
> PluginResolver for WasmPluginResolver<TEnvironment, TCompileFn, TImportObjectFactory> {
    async fn resolve_plugins(&self, path_sources: Vec<PathSource>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
        let mut handles = Vec::with_capacity(path_sources.len());
        let mut plugins = Vec::with_capacity(path_sources.len());

        for path_source in path_sources.into_iter() {
            let environment = self.environment.clone();
            let plugin_cache = self.plugin_cache.clone();
            let import_object_factory = self.import_object_factory.clone();
            handles.push(tokio::task::spawn(async move {
                match resolve_plugin(import_object_factory, &plugin_cache, environment, &path_source).await {
                    Ok(plugin) => Ok(plugin),
                    Err(err) => {
                        match plugin_cache.forget(&path_source) {
                            Ok(()) => {},
                            Err(inner_err) => return err!("Error resolving plugin {} and forgetting from cache: {}\n{}", path_source.display(), err, inner_err),
                        }
                        return err!("Error resolving plugin {}: {}", path_source.display(), err);
                    }
                }
            }));
        }

        let result = futures::future::try_join_all(handles).await?;
        for plugin_result in result {
            plugins.push(plugin_result?);
        }

        Ok(plugins)
    }
}

impl<
    TEnvironment : Environment,
    TCompileFn : CompileFn,
    TImportObjectFactory : ImportObjectFactory,
> WasmPluginResolver<TEnvironment, TCompileFn, TImportObjectFactory> {
    pub fn new(
        environment: TEnvironment,
        plugin_cache: PluginCache<TEnvironment, TCompileFn>,
        import_object_factory: TImportObjectFactory,
    ) -> Self {
        WasmPluginResolver { environment, plugin_cache, import_object_factory }
    }
}

async fn resolve_plugin<TEnvironment : Environment, TCompileFn : CompileFn, TImportObjectFactory : ImportObjectFactory>(
    import_object_factory: TImportObjectFactory,
    plugin_cache: &PluginCache<TEnvironment, TCompileFn>,
    environment: TEnvironment,
    path_source: &PathSource,
) -> Result<Box<dyn Plugin>, ErrBox> {
    let cache_item = plugin_cache.get_plugin_cache_item(path_source).await;
    let cache_item: PluginCacheItem = match cache_item {
        Ok(cache_item) => Ok(cache_item),
        Err(err) => {
            environment.log_error(&format!(
                "Error getting plugin from cache. Forgetting from cache and retrying. Message: {:?}",
                err
            ));

            // forget and try again
            plugin_cache.forget(path_source)?;
            plugin_cache.get_plugin_cache_item(path_source).await
        }
    }?;
    let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
        Ok(file_bytes) => file_bytes,
        Err(err) => {
            environment.log_error(&format!(
                "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {:?}",
                err
            ));

            // forget and try again
            plugin_cache.forget(path_source)?;
            let cache_item = plugin_cache.get_plugin_cache_item(path_source).await?;
            environment.read_file_bytes(&cache_item.file_path)?
        }
    };

    Ok(Box::new(WasmPlugin::new(file_bytes, cache_item.info, import_object_factory)))
}
