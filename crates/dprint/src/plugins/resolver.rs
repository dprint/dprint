use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use dprint_core::async_runtime::future;
use dprint_core::communication::IdGenerator;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::PluginInfo;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use super::InitializedPlugin;
use super::implementations::WasmModuleCreator;
use super::implementations::create_plugin;
use crate::environment::Environment;
use crate::plugins::Plugin;
use crate::plugins::PluginCache;
use crate::plugins::PluginSourceReference;
use crate::utils::AsyncCell;
use crate::utils::PathSource;

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
    self.initialized_plugin.get_or_try_init(|| self.plugin.initialize()).await.cloned()
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
  /// Whether to download remote plugins again instead of using the cached
  /// ones (`--reload`).
  reload_plugins: bool,
  /// The plugins downloaded again, so each is only reloaded once per process.
  reloaded: RefCell<HashSet<PluginSourceReference>>,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: PluginCache<TEnvironment>, reload_plugins: bool) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      memory_cache: Default::default(),
      wasm_module_creator: Default::default(),
      next_config_id: Default::default(),
      reload_plugins,
      reloaded: Default::default(),
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

  /// Sets up a versioned npm plugin for `dprint add` — downloads, resolves the
  /// plugin file (detecting it when the specifier has no path), computes the
  /// checksum, and warms the cache — returning the resolved path + checksum to
  /// write into config. See [`PluginCache::resolve_npm_for_add`].
  pub async fn resolve_npm_for_add(
    &self,
    specifier: &crate::utils::NpmSpecifier,
    path_was_explicit: bool,
    base_dir: Option<&crate::environment::CanonicalizedPathBuf>,
  ) -> Result<crate::plugins::NpmAddResolution> {
    self.plugin_cache.resolve_npm_for_add(specifier, path_was_explicit, base_dir).await
  }

  /// Downloads a remote plugin for `dprint add`, computes its checksum, and
  /// warms the cache. See [`PluginCache::resolve_remote_for_add`].
  pub async fn resolve_remote_for_add(&self, plugin_reference: &PluginSourceReference) -> Result<String> {
    self.plugin_cache.resolve_remote_for_add(plugin_reference).await
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
        if self.should_reload(&plugin_reference) {
          self
            .plugin_cache
            .forget(&plugin_reference)
            .await
            .with_context(|| format!("Error forgetting plugin {} from the cache to reload it", plugin_reference.display()))?;
        }
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
            Err(err).with_context(|| format!("Error resolving plugin {}", plugin_reference.display()))
          }
        }
      })
      .await
      .cloned()
  }

  /// Whether the plugin should be downloaded again, which is only once per
  /// process even when the plugins are shut down and resolved again (ex. the
  /// language server after a config change). Local plugins are already
  /// checked for changes.
  fn should_reload(&self, plugin_reference: &PluginSourceReference) -> bool {
    self.reload_plugins && matches!(plugin_reference.path_source, PathSource::Remote(_)) && self.reloaded.borrow_mut().insert(plugin_reference.clone())
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironmentBuilder;

  #[test]
  fn should_reload_remote_plugin_once_per_process() {
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let url = "https://plugins.dprint.dev/test-plugin.wasm";
        let reference = PluginSourceReference {
          path_source: PathSource::new_remote(url.parse().unwrap()),
          checksum: None,
        };

        // cached by a previous process
        let resolver = Rc::new(PluginResolver::new(environment.clone(), PluginCache::new(environment.clone()), false));
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 1);
        resolver.clear_and_shutdown_initialized().await;

        // reloading downloads it again, but only once in the process
        let resolver = Rc::new(PluginResolver::new(environment.clone(), PluginCache::new(environment.clone()), true));
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 2);
        resolver.clear_and_shutdown_initialized().await;
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 2);
        resolver.clear_and_shutdown_initialized().await;

        // not reloading uses the cache
        let resolver = Rc::new(PluginResolver::new(environment.clone(), PluginCache::new(environment.clone()), false));
        resolver.resolve_plugins(vec![reference]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 2);
        resolver.clear_and_shutdown_initialized().await;
      }
    });
    // compiled for the first download and the reload only
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Compiling https://plugins.dprint.dev/test-plugin.wasm",
        "Compiling https://plugins.dprint.dev/test-plugin.wasm"
      ]
    );
  }
}
