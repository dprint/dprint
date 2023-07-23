use anyhow::bail;
use anyhow::Result;
use dprint_core::async_runtime::future;
use dprint_core::communication::IdGenerator;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::PluginInfo;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::implementations::create_plugin;
use super::implementations::WasmModuleCreator;
use super::InitializedPlugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::utils::AsyncCell;

pub struct PluginWrapper {
  plugin: Box<dyn Plugin>,
  initialized_plugin: AsyncCell<Rc<dyn InitializedPlugin>>,
}

impl PluginWrapper {
  pub fn new(plugin: Box<dyn Plugin>) -> Self {
    Self {
      plugin,
      initialized_plugin: Default::default(),
    }
  }

  pub fn info(&self) -> &PluginInfo {
    self.plugin.info()
  }

  pub fn is_process_plugin(&self) -> bool {
    self.plugin.is_process_plugin()
  }

  pub async fn initialize(&self) -> Result<Rc<dyn InitializedPlugin>> {
    self.initialized_plugin.get_or_try_init(|| self.plugin.initialize()).await.map(|x| x.clone())
  }

  pub async fn shutdown(&self) {
    if let Some(plugin) = self.initialized_plugin.get() {
      plugin.shutdown().await;
    }
  }
}

pub struct PluginResolver<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_cache: PluginCache<TEnvironment>,
  memory_cache: RefCell<HashMap<PluginSourceReference, Rc<tokio::sync::OnceCell<Rc<PluginWrapper>>>>>,
  wasm_module_creator: WasmModuleCreator,
  next_config_id: IdGenerator,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: PluginCache<TEnvironment>) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      memory_cache: Default::default(),
      wasm_module_creator: Default::default(),
      next_config_id: Default::default(),
    }
  }

  pub async fn clear_and_shutdown_initialized(&self) {
    let plugins = self.memory_cache.borrow_mut().drain().collect::<Vec<_>>();
    let futures = plugins.iter().filter_map(|p| p.1.get()).map(|p| p.shutdown());
    future::join_all(futures).await;
  }

  pub fn next_config_id(&self) -> FormatConfigId {
    // + 1 because 0 is reserved for uninitialized
    FormatConfigId::from_raw(self.next_config_id.next() + 1)
  }

  pub async fn resolve_plugins(self: &Rc<Self>, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Rc<PluginWrapper>>> {
    let handles = plugin_references
      .into_iter()
      .map(|plugin_ref| {
        let resolver = self.clone();
        dprint_core::async_runtime::spawn(async move { resolver.resolve_plugin(plugin_ref).await })
      })
      .collect::<Vec<_>>();

    let results = future::join_all(handles).await;
    let mut plugins = Vec::with_capacity(results.len());
    for result in results {
      plugins.push(result??);
    }

    Ok(plugins)
  }

  pub async fn resolve_plugin(&self, plugin_reference: PluginSourceReference) -> Result<Rc<PluginWrapper>> {
    let cell = {
      let mut mem_cache = self.memory_cache.borrow_mut();
      mem_cache
        .entry(plugin_reference.clone())
        .or_insert_with(|| Rc::new(tokio::sync::OnceCell::new()))
        .clone()
    };
    cell
      .get_or_try_init(|| async {
        match create_plugin(&self.plugin_cache, self.environment.clone(), &plugin_reference, &self.wasm_module_creator).await {
          Ok(plugin) => Ok(Rc::new(PluginWrapper::new(plugin))),
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
