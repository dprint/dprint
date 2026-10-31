use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use dprint_core::async_runtime::future;
use dprint_core::communication::IdGenerator;
use dprint_core::plugins::FileMatchingInfo;
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
use crate::utils::UntrustedDownloadError;

pub struct PluginWrapper {
  plugin: Box<dyn Plugin>,
  initialized_plugin: AsyncCell<Rc<dyn InitializedPlugin>>,
  was_set_up: bool,
}

impl PluginWrapper {
  pub fn new(plugin: Box<dyn Plugin>) -> Self {
    Self {
      plugin,
      initialized_plugin: Default::default(),
      was_set_up: false,
    }
  }

  /// Whether this process set the plugin up (downloaded, compiled, or
  /// extracted it) instead of it already being in the cache.
  pub fn was_set_up(&self) -> bool {
    self.was_set_up
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

#[derive(Debug, Clone, Copy, Default)]
pub struct PluginResolverOptions {
  /// Whether to download remote plugins again instead of using the cached
  /// ones (`--reload`).
  pub reload_plugins: bool,
}

pub struct PluginResolver<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_cache: PluginCache<TEnvironment>,
  memory_cache: RefCell<HashMap<PluginSourceReference, Rc<tokio::sync::OnceCell<Rc<PluginWrapper>>>>>,
  wasm_module_creator: WasmModuleCreator,
  next_config_id: IdGenerator,
  options: PluginResolverOptions,
  /// The plugins downloaded again, so each is only reloaded once per process.
  reloaded: RefCell<HashSet<PluginSourceReference>>,
  /// The config keys of the process plugins warned about having untrusted
  /// configuration, so each is only warned about once per process.
  warned_untrusted_config_keys: RefCell<HashSet<String>>,
}

impl<TEnvironment: Environment> PluginResolver<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_cache: PluginCache<TEnvironment>, options: PluginResolverOptions) -> Self {
    PluginResolver {
      environment,
      plugin_cache,
      memory_cache: Default::default(),
      wasm_module_creator: Default::default(),
      next_config_id: Default::default(),
      options,
      reloaded: Default::default(),
      warned_untrusted_config_keys: Default::default(),
    }
  }

  /// Shuts down and forgets the plugins that aren't one of the used ones.
  pub async fn shutdown_unused(&self, used_plugins: &[Rc<PluginWrapper>]) {
    let unused_plugins = {
      let mut memory_cache = self.memory_cache.borrow_mut();
      let unused_references = memory_cache
        .iter()
        .filter(|(_, cell)| cell.get().is_some_and(|plugin| !used_plugins.iter().any(|used| Rc::ptr_eq(used, plugin))))
        .map(|(reference, _)| reference.clone())
        .collect::<Vec<_>>();
      unused_references
        .into_iter()
        .filter_map(|reference| memory_cache.remove(&reference))
        .collect::<Vec<_>>()
    };
    let futures = unused_plugins.iter().filter_map(|plugin| plugin.get()).map(|plugin| plugin.shutdown());
    future::join_all(futures).await;
  }

  pub async fn clear_and_shutdown_initialized(&self) {
    let plugins = self.memory_cache.borrow_mut().drain().collect::<Vec<_>>();
    let futures = plugins.iter().filter_map(|p| p.1.get()).map(|p| p.shutdown());
    future::join_all(futures).await;
  }

  /// Gets whether to warn that the untrusted configuration of the process
  /// plugin with the config key was ignored, which is only the first time.
  pub fn should_warn_untrusted_config(&self, config_key: &str) -> bool {
    self.warned_untrusted_config_keys.borrow_mut().insert(config_key.to_string())
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

  /// Whether the plugin can be used without downloading it (see
  /// `PluginCache::is_installed`).
  pub fn is_installed(&self, plugin_reference: &PluginSourceReference) -> bool {
    self.get_resolved_plugin_info(plugin_reference).is_some() || self.plugin_cache.is_installed(plugin_reference)
  }

  /// Gets the information of the plugin when it's set up without setting it
  /// up (which runs a process plugin to get its information).
  pub fn get_set_up_plugin_info(&self, plugin_reference: &PluginSourceReference) -> Option<PluginInfo> {
    self
      .get_resolved_plugin_info(plugin_reference)
      .or_else(|| self.plugin_cache.get_set_up_plugin_info(plugin_reference))
  }

  /// Gets the files the plugin matched the last time an editor resolved the
  /// config file with it, which doesn't set it up.
  pub fn get_last_file_matching_info(&self, plugin_reference: &PluginSourceReference, config: &str) -> Option<FileMatchingInfo> {
    self.plugin_cache.get_last_file_matching_info(plugin_reference, config)
  }

  /// Remembers the files the plugin matched when an editor resolved the config
  /// file with it.
  pub async fn store_file_matching_info(&self, plugin_reference: &PluginSourceReference, config: &str, file_matching: &FileMatchingInfo) -> Result<()> {
    self.plugin_cache.store_file_matching_info(plugin_reference, config, file_matching).await
  }

  fn get_resolved_plugin_info(&self, plugin_reference: &PluginSourceReference) -> Option<PluginInfo> {
    let memory_cache = self.memory_cache.borrow();
    let plugin = memory_cache.get(plugin_reference)?.get()?;
    Some(plugin.info().clone())
  }

  /// Gets the npm registry the plugin is from when it isn't the default one
  /// (see `PluginCache::get_npm_registry`).
  pub fn get_npm_registry(&self, plugin_reference: &PluginSourceReference) -> Option<String> {
    self.plugin_cache.get_npm_registry(plugin_reference)
  }

  /// Gets the url the plugin is downloaded from (see
  /// `PluginCache::get_download_url`).
  pub fn get_download_url(&self, plugin_reference: &PluginSourceReference) -> Result<Option<url::Url>, UntrustedDownloadError> {
    self.plugin_cache.get_download_url(plugin_reference)
  }

  /// Gets what the plugin needs to download that the download policy doesn't
  /// allow without asking the user (see `UntrustedDownloadError`), which is the
  /// url it's downloaded from and the proxy of an `.npmrc` file that it's
  /// downloaded through (see `NpmrcProxy`), or the `.npmrc` file on another
  /// machine that needs to be read to know the url (see `NpmrcChain::load`).
  pub fn get_disallowed_downloads(&self, plugin_reference: &PluginSourceReference) -> Vec<String> {
    if self.is_installed(plugin_reference) {
      return Vec::new();
    }
    let download_policy = self.environment.download_policy();
    let mut downloads = Vec::new();
    let is_npm = matches!(plugin_reference.path_source, PathSource::Npm(_));
    match self.get_download_url(plugin_reference) {
      // a registry allows each package from it, unlike a url
      Ok(Some(url)) if is_npm => {
        if let Err(err) = download_policy.ensure_allows_npm_registry(&url) {
          downloads.push(err.download);
        }
      }
      Ok(Some(url)) if !download_policy.allows(&url) => downloads.push(url.to_string()),
      Ok(_) => {}
      Err(err) => downloads.push(err.download),
    }
    if let Some(proxy) = self
      .plugin_cache
      .get_download_npmrc_proxy(plugin_reference)
      .filter(|proxy| !download_policy.allows_npmrc_proxy(proxy))
    {
      downloads.push(proxy.to_download());
    }
    downloads
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
          Ok(created) => Ok(Rc::new(PluginWrapper {
            was_set_up: created.was_set_up,
            ..PluginWrapper::new(created.plugin)
          })),
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
    self.options.reload_plugins && matches!(plugin_reference.path_source, PathSource::Remote(_)) && self.reloaded.borrow_mut().insert(plugin_reference.clone())
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironmentBuilder;

  #[cfg(windows)]
  #[test]
  fn should_need_reading_an_npmrc_on_another_machine_allowed_to_know_where_an_npm_plugin_is_downloaded_from() {
    use crate::environment::TestEnvironment;
    use crate::utils::DownloadPolicy;
    use crate::utils::NpmSpecifier;
    use std::path::Path;
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/repo").unwrap();
    // ex. a file that sets a company's registry for its scope, which isn't in the test file system
    environment.symlink(r"\\server\share\dir\npmrc", "/repo/.npmrc").unwrap();
    environment.set_download_policy(DownloadPolicy::for_editor(&environment));
    let resolver = PluginResolver::new(environment.clone(), PluginCache::new(environment.clone()), PluginResolverOptions::default());
    let reference = PluginSourceReference {
      path_source: PathSource::new_npm(
        NpmSpecifier {
          name: "@acme/plugin".to_string(),
          version: Some("1.0.0".to_string()),
          path: "plugin.wasm".to_string(),
        },
        Some(environment.canonicalize("/repo").unwrap()),
      ),
      checksum: None,
    };
    let npmrc_download = format!(r"{} -> \\server\share\dir\npmrc", Path::new("/repo").join(".npmrc").display());
    // it isn't downloaded from the default registry without reading the file that may set another
    assert_eq!(resolver.get_disallowed_downloads(&reference), vec![npmrc_download.clone()]);
    assert_eq!(resolver.get_npm_registry(&reference), None);
    // and then it's downloaded from the registry the file sets, which is the default one when it doesn't exist
    environment.download_policy().allow(&[npmrc_download]);
    assert_eq!(resolver.get_disallowed_downloads(&reference), Vec::<String>::new());
    assert_eq!(resolver.get_npm_registry(&reference), None);
    assert_eq!(environment.file_read_count("/repo/.npmrc"), 1);
  }

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
        let resolver = Rc::new(PluginResolver::new(
          environment.clone(),
          PluginCache::new(environment.clone()),
          PluginResolverOptions::default(),
        ));
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 1);
        resolver.clear_and_shutdown_initialized().await;

        // reloading downloads it again, but only once in the process
        let resolver = Rc::new(PluginResolver::new(
          environment.clone(),
          PluginCache::new(environment.clone()),
          PluginResolverOptions { reload_plugins: true },
        ));
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 2);
        resolver.clear_and_shutdown_initialized().await;
        resolver.resolve_plugins(vec![reference.clone()]).await.unwrap();
        assert_eq!(environment.remote_file_request_count(url), 2);
        resolver.clear_and_shutdown_initialized().await;

        // not reloading uses the cache
        let resolver = Rc::new(PluginResolver::new(
          environment.clone(),
          PluginCache::new(environment.clone()),
          PluginResolverOptions::default(),
        ));
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
