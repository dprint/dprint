use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::{Plugin, PluginSourceReference, PluginCache, PluginCacheItem};
use crate::plugins::process::ProcessPlugin;
use crate::plugins::wasm::{WasmPlugin, PoolImportObjectFactory};

pub struct PluginResolver<TEnvironment : Environment> {
    environment: TEnvironment,
    plugin_cache: PluginCache<TEnvironment>,
    import_object_factory: PoolImportObjectFactory<TEnvironment>,
}

impl<TEnvironment : Environment> PluginResolver<TEnvironment> {
    pub fn new(
        environment: TEnvironment,
        plugin_cache: PluginCache<TEnvironment>,
        import_object_factory: PoolImportObjectFactory<TEnvironment>,
    ) -> Self {
        PluginResolver { environment, plugin_cache, import_object_factory }
    }

    pub async fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
        let mut handles = Vec::with_capacity(plugin_references.len());
        let mut plugins = Vec::with_capacity(plugin_references.len());

        for plugin_reference in plugin_references.into_iter() {
            let environment = self.environment.clone();
            let plugin_cache = self.plugin_cache.clone();
            let import_object_factory = self.import_object_factory.clone();
            handles.push(tokio::task::spawn(async move {
                match resolve_plugin(import_object_factory, &plugin_cache, environment, &plugin_reference).await {
                    Ok(plugin) => Ok(plugin),
                    Err(err) => {
                        match plugin_cache.forget(&plugin_reference) {
                            Ok(()) => {},
                            Err(inner_err) => return err!("Error resolving plugin {} and forgetting from cache: {}\n{}", plugin_reference.display(), err, inner_err),
                        }
                        return err!("Error resolving plugin {}: {}", plugin_reference.display(), err);
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

async fn resolve_plugin<TEnvironment : Environment>(
    import_object_factory: PoolImportObjectFactory<TEnvironment>,
    plugin_cache: &PluginCache<TEnvironment>,
    environment: TEnvironment,
    plugin_reference: &PluginSourceReference,
) -> Result<Box<dyn Plugin>, ErrBox> {
    let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await;
    let cache_item: PluginCacheItem = match cache_item {
        Ok(cache_item) => Ok(cache_item),
        Err(err) => {
            environment.log_error(&format!(
                "Error getting plugin from cache. Forgetting from cache and retrying. Message: {:?}",
                err
            ));

            // forget and try again
            plugin_cache.forget(plugin_reference)?;
            plugin_cache.get_plugin_cache_item(plugin_reference).await
        }
    }?;

    // todo: consolidate with setup_plugin.rs so all code like this is in the same place
    if plugin_reference.is_wasm_plugin() {
        let file_bytes = match environment.read_file_bytes(&cache_item.file_path) {
            Ok(file_bytes) => file_bytes,
            Err(err) => {
                environment.log_error(&format!(
                    "Error reading plugin file bytes. Forgetting from cache and retrying. Message: {:?}",
                    err
                ));

                // forget and try again
                plugin_cache.forget(plugin_reference)?;
                let cache_item = plugin_cache.get_plugin_cache_item(plugin_reference).await?;
                environment.read_file_bytes(&cache_item.file_path)?
            }
        };

        Ok(Box::new(WasmPlugin::new(file_bytes, cache_item.info, import_object_factory)))
    } else if plugin_reference.is_process_plugin() {
        let cache_item = if !environment.path_exists(&cache_item.file_path) {
            environment.log_error(&format!(
                "Could not find process plugin at {}. Forgetting from cache and retrying.",
                cache_item.file_path.display()
            ));

            // forget and try again
            plugin_cache.forget(plugin_reference)?;
            plugin_cache.get_plugin_cache_item(plugin_reference).await?
        } else {
            cache_item
        };

        Ok(Box::new(ProcessPlugin::new(cache_item.info, cache_item.file_path)))
    } else {
        return err!("Could not resolve plugin type from url or file path: {}", plugin_reference.display());
    }
}
