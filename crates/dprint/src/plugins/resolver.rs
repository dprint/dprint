use std::sync::Arc;
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::plugins::{Plugin, PluginSourceReference, PluginCache, PluginPools};
use super::implementations::{create_plugin};

pub struct PluginResolver<TEnvironment : Environment> {
    environment: TEnvironment,
    plugin_cache: Arc<PluginCache<TEnvironment>>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment : Environment> PluginResolver<TEnvironment> {
    pub fn new(
        environment: TEnvironment,
        plugin_cache: Arc<PluginCache<TEnvironment>>,
        plugin_pools: Arc<PluginPools<TEnvironment>>,
    ) -> Self {
        PluginResolver { environment, plugin_cache, plugin_pools }
    }

    pub async fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
        let mut handles = Vec::with_capacity(plugin_references.len());
        let mut plugins = Vec::with_capacity(plugin_references.len());

        for plugin_reference in plugin_references.into_iter() {
            let environment = self.environment.clone();
            let plugin_cache = self.plugin_cache.clone();
            let plugin_pools = self.plugin_pools.clone();
            handles.push(tokio::task::spawn(async move {
                match create_plugin(plugin_pools, &plugin_cache, environment, &plugin_reference).await {
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
