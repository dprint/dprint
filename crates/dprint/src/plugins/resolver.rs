use anyhow::bail;
use anyhow::Result;
use std::sync::Arc;

use super::implementations::create_plugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginsCollection;

#[derive(Clone)]
pub struct PluginResolver<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_cache: Arc<PluginCache<TEnvironment>>,
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: Arc<PluginCache<TEnvironment>>, plugins_collection: Arc<PluginsCollection<TEnvironment>>) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      plugins_collection,
    }
  }

  pub async fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>> {
    let handles = plugin_references
      .into_iter()
      .map(|plugin_ref| {
        let resolver = self.clone();
        tokio::task::spawn(async move { resolver.resolve_plugin(&plugin_ref).await })
      })
      .collect::<Vec<_>>();

    let results = futures::future::join_all(handles).await;
    let mut plugins = Vec::with_capacity(results.len());
    for result in results {
      plugins.push(result??);
    }

    Ok(plugins)
  }

  pub async fn resolve_plugin(&self, plugin_reference: &PluginSourceReference) -> Result<Box<dyn Plugin>> {
    match create_plugin(self.plugins_collection.clone(), &self.plugin_cache, self.environment.clone(), plugin_reference).await {
      Ok(plugin) => Ok(plugin),
      Err(err) => {
        match self.plugin_cache.forget(plugin_reference) {
          Ok(()) => {}
          Err(inner_err) => {
            bail!(
              "Error resolving plugin {} and forgetting from cache: {}\n{}",
              plugin_reference.display(),
              err,
              inner_err
            )
          }
        }
        bail!("Error resolving plugin {}: {}", plugin_reference.display(), err);
      }
    }
  }
}
