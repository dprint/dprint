use anyhow::bail;
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::implementations::create_plugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginsCollection;

pub struct PluginResolver<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_cache: Arc<PluginCache<TEnvironment>>,
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  memory_cache: Mutex<HashMap<PluginSourceReference, Arc<tokio::sync::OnceCell<Arc<dyn Plugin>>>>>,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: Arc<PluginCache<TEnvironment>>, plugins_collection: Arc<PluginsCollection<TEnvironment>>) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      plugins_collection,
      memory_cache: Default::default(),
    }
  }

  pub async fn resolve_plugins(self: &Arc<Self>, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Arc<dyn Plugin>>> {
    let handles = plugin_references
      .into_iter()
      .map(|plugin_ref| {
        let resolver = self.clone();
        tokio::task::spawn(async move { resolver.resolve_plugin(plugin_ref).await })
      })
      .collect::<Vec<_>>();

    let results = futures::future::join_all(handles).await;
    let mut plugins = Vec::with_capacity(results.len());
    for result in results {
      plugins.push(result??);
    }

    Ok(plugins)
  }

  pub async fn resolve_plugin(&self, plugin_reference: PluginSourceReference) -> Result<Arc<dyn Plugin>> {
    let cell = {
      let mut mem_cache = self.memory_cache.lock();
      mem_cache
        .entry(plugin_reference.clone())
        .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
        .clone()
    };
    let cache = self.plugin_cache.clone();
    let environment = self.environment.clone();
    cell
      .get_or_try_init(|| async {
        match create_plugin(&self.plugin_cache, self.environment.clone(), &plugin_reference).await {
          Ok(plugin) => Ok(plugin),
          Err(err) => {
            match self.plugin_cache.forget(&plugin_reference).await {
              Ok(()) => {}
              Err(inner_err) => {
                bail!(
                  "Error resolving plugin {} and forgetting from cache: {:#}\n{:#}",
                  plugin_reference.display(),
                  err,
                  inner_err
                )
              }
            }
            bail!("Error resolving plugin {}: {:#}", plugin_reference.display(), err);
          }
        }
      })
      .await
      .map(|p| p.clone())
  }
}
