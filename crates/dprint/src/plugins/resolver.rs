use anyhow::bail;
use anyhow::Result;
use std::sync::Arc;

use super::implementations::create_plugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginPools;
use crate::plugins::PluginSourceReference;

#[derive(Clone)]
pub struct PluginResolver<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_cache: Arc<PluginCache<TEnvironment>>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: Arc<PluginCache<TEnvironment>>, plugin_pools: Arc<PluginPools<TEnvironment>>) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      plugin_pools,
    }
  }

  pub async fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>> {
    let handles = plugin_references
      .into_iter()
      .map(|plugin_ref| {
        let resolver = self.clone();
        tokio::task::spawn_blocking(move || {
          // todo: make this async
          resolver.resolve_plugin(&plugin_ref)
        })
      })
      .collect::<Vec<_>>();

    let results = futures::future::join_all(handles).await;
    let mut plugins = Vec::with_capacity(results.len());
    for result in results {
      plugins.push(result??);
    }

    Ok(plugins)
  }

  pub fn resolve_plugin(&self, plugin_reference: &PluginSourceReference) -> Result<Box<dyn Plugin>> {
    match create_plugin(self.plugin_pools.clone(), &self.plugin_cache, self.environment.clone(), plugin_reference) {
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
