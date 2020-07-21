use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::{Plugin, PluginSourceReference, PluginCache, create_plugin};
use crate::plugins::wasm::PoolImportObjectFactory;

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
                match create_plugin(import_object_factory, &plugin_cache, environment, &plugin_reference).await {
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
