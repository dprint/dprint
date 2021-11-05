use rayon::prelude::*;
use std::sync::Arc;

use dprint_core::types::ErrBox;

use super::implementations::create_plugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginPools;
use crate::plugins::PluginSourceReference;

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

  pub fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    let plugins = plugin_references
      .into_par_iter()
      .map(|plugin_reference| self.resolve_plugin(&plugin_reference))
      .collect::<Result<Vec<Box<dyn Plugin>>, ErrBox>>()?;

    Ok(plugins)
  }

  pub fn resolve_plugin(&self, plugin_reference: &PluginSourceReference) -> Result<Box<dyn Plugin>, ErrBox> {
    match create_plugin(self.plugin_pools.clone(), &self.plugin_cache, self.environment.clone(), plugin_reference) {
      Ok(plugin) => Ok(plugin),
      Err(err) => {
        match self.plugin_cache.forget(plugin_reference) {
          Ok(()) => {}
          Err(inner_err) => {
            return err!(
              "Error resolving plugin {} and forgetting from cache: {}\n{}",
              plugin_reference.display(),
              err,
              inner_err
            )
          }
        }
        return err!("Error resolving plugin {}: {}", plugin_reference.display(), err);
      }
    }
  }
}
