use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use dprint_core::communication::AtomicFlag;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::Host;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::PluginInfo;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use indexmap::IndexMap;
use thiserror::Error;

use crate::arg_parser::CliArgs;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::get_global_config;
use crate::configuration::get_plugin_config_map;
use crate::configuration::resolve_config_from_args;
use crate::configuration::resolve_config_from_path;
use crate::configuration::GetGlobalConfigOptions;
use crate::configuration::RawPluginConfig;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPath;
use crate::environment::Environment;
use crate::paths::get_and_resolve_file_paths;
use crate::paths::get_file_paths_by_plugins_and_err_if_empty;
use crate::paths::PluginNames;
use crate::plugins::output_plugin_config_diagnostics;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::Plugin;
use crate::plugins::PluginNameResolutionMaps;
use crate::plugins::PluginResolver;
use crate::utils::ErrorCountLogger;
use crate::utils::ResolvedPath;

pub struct PluginWrapper {
  plugin: Arc<dyn Plugin>,
  initialized_plugin: tokio::sync::OnceCell<Arc<dyn InitializedPlugin>>,
}

impl PluginWrapper {
  pub fn info(&self) -> &PluginInfo {
    self.plugin.info()
  }

  pub async fn initialize(&self) -> Result<Arc<dyn InitializedPlugin>> {
    self.initialized_plugin.get_or_try_init(|| async move { Ok(self.plugin.initialize().await?) })
  }
}

enum GetPluginResult {
  HadDiagnostics(usize),
  Success(Arc<dyn InitializedPlugin>),
}

pub struct PluginWithConfig {
  pub plugin: PluginWrapper,
  pub associations: Option<Vec<String>>,
  pub format_config: Arc<FormatConfig>,
  config_diagnostic_count: tokio::sync::Mutex<Option<usize>>,
}

impl PluginWithConfig {
  /// Gets a hash that represents the current state of the plugin.
  /// This is used for the "incremental" feature to tell if a plugin has changed state.
  pub fn get_hash(&self) -> u64 {
    // todo: update this to be passed a hasher
    let mut hash_str = String::new();

    // list everything in here that would affect formatting
    hash_str.push_str(&self.info().name);
    hash_str.push_str(&self.info().version);

    // serialize the config keys in order to prevent the hash from changing
    let sorted_config = self.format_config.raw.iter().collect::<BTreeMap<_, _>>();
    hash_str.push_str(&serde_json::to_string(&sorted_config).unwrap());

    hash_str.push_str(&serde_json::to_string(&self.associations).unwrap());
    hash_str.push_str(&serde_json::to_string(&self.global_config).unwrap());

    crate::utils::get_bytes_hash(hash_str.as_bytes())
  }

  pub fn info(&self) -> &PluginInfo {
    self.plugin.info()
  }

  async fn get_or_create_checking_config_diagnostics<TEnvironment: Environment>(&self, environment: &TEnvironment) -> Result<GetPluginResult> {
    // only allow one thread to initialize and output the diagnostics (we don't want the messages being spammed)
    let instance = self.plugin.initialize().await?;
    let config_diagnostic_count = self.config_diagnostic_count.lock();
    match *config_diagnostic_count {
      Some(count) => {
        if count > 0 {
          return Ok(GetPluginResult::HadDiagnostics(count));
        }
        Ok(GetPluginResult::Success(instance))
      }
      None => {
        let result = output_plugin_config_diagnostics(&self.info().name, &instance, self.format_config.clone(), environment).await;
        *self.config_diagnostic_count.lock() = Some(result.is_ok());
        if let Err(err) = result {
          environment.log_stderr(&err.to_string());
          instance.shutdown().await;
          return Ok(GetPluginResult::HadDiagnostics(err.diagnostic_count));
        }
      }
    }
  }
}

#[derive(Default)]
pub struct PluginsScope<TEnvironment: Environment> {
  environment: TEnvironment,
  pub plugins: IndexMap<String, Arc<PluginWithConfig>>,
  pub plugin_name_maps: PluginNameResolutionMaps,
}

impl<TEnvironment: Environment> PluginsScope<TEnvironment> {
  fn get_plugin(&self, name: &str) -> Arc<PluginWithConfig> {
    self
      .plugins
      .get(name)
      .cloned()
      .unwrap_or_else(|| panic!("Expected to find plugin in collection: {}", name))
  }
}

impl<TEnvironment: Environment> Host for PluginsScope<TEnvironment> {
  fn format(&self, request: HostFormatRequest) -> dprint_core::plugins::BoxFuture<FormatResult> {
    let plugin_names = self.plugin_name_maps.get_plugin_names_from_file_path(&request.file_path);
    log_verbose!(
      self.environment,
      "Host formatting {} - File length: {} - Plugins: [{}] - Range: {:?}",
      request.file_path.display(),
      request.file_text.len(),
      plugin_names.join(", "),
      request.range,
    );
    async move {
      let mut file_text = request.file_text;
      let mut had_change = false;
      for plugin_name in plugin_names {
        let plugin = self.get_plugin(&plugin_name);
        match plugin.get_or_create_checking_config_diagnostics(&self.environment).await {
          Ok(GetPluginResult::Success(initialized_plugin)) => {
            let result = initialized_plugin
              .format_text(InitializedPluginFormatRequest {
                file_path: request.file_path.clone(),
                file_text: file_text.clone(),
                range: request.range.clone(),
                config: plugin.format_config.clone(),
                override_config: request.override_config.clone(),
                token: request.token.clone(),
              })
              .await;
            if let Some(new_text) = result? {
              file_text = new_text;
              had_change = true;
            }
          }
          Ok(GetPluginResult::HadDiagnostics(count)) => bail!("Had {} configuration errors.", count),
          Err(err) => return Err(CriticalFormatError(err).into()),
        }
      }

      Ok(if had_change { Some(file_text) } else { None })
    }
    .boxed()
  }
}

pub struct PluginsAndPaths<TEnvironment: Environment> {
  pub scope: PluginsScope<TEnvironment>,
  pub file_paths_by_plugins: HashMap<PluginNames, Vec<PathBuf>>,
}

pub async fn resolve_plugins_and_paths<TEnvironment: Environment>(
  args: &CliArgs,
  patterns: &FilePatternArgs,
  environment: &TEnvironment,
  plugin_resolver: &Arc<PluginResolver<TEnvironment>>,
) -> Result<PluginsAndPaths<TEnvironment>> {
  let resolve_plugins_options = ResolvePluginsOptions {
    // Skip checking these diagnostics when the user provides
    // plugins from the CLI args. They may be doing this to filter
    // to only specific plugins.
    check_top_level_unknown_property_diagnostics: args.plugins.is_empty(),
  };
  let resolver = PluginsAndPathsResolver {
    args,
    patterns,
    environment,
    plugin_resolver,
    resolve_plugins_options: &resolve_plugins_options,
  };

  resolver.resolve_config().await
}

struct PluginsAndPathsResolver<'a, TEnvironment: Environment> {
  args: &'a CliArgs,
  patterns: &'a FilePatternArgs,
  environment: &'a TEnvironment,
  plugin_resolver: &'a Arc<PluginResolver<TEnvironment>>,
  resolve_plugins_options: &'a ResolvePluginsOptions,
}

impl<'a, TEnvironment: Environment> PluginsAndPathsResolver<'a, TEnvironment> {
  pub async fn resolve_config(&self) -> Result<PluginsAndPaths<TEnvironment>> {
    let config = resolve_config_from_args(self.args, self.environment)?;
    let scope = resolve_plugins_scope_and_err_if_empty(&config, self.environment, self.plugin_resolver, self.resolve_plugins_options).await?;
    let glob_output = get_and_resolve_file_paths(&config, self.patterns, &scope.plugins, self.environment).await?;
    let file_paths_by_plugins = get_file_paths_by_plugins_and_err_if_empty(&scope.plugin_name_maps, glob_output.file_paths)?;

    let mut result = vec![PluginsAndPaths { scope, file_paths_by_plugins }];
    for config_file_path in glob_output.config_files {
      result.extend(self.resolve_sub_config(config_file_path, &config).await?);
    }

    // todo: have this return the entire vector... just doing the first one for now to reduce compiler errors while refactoring
    Ok(result.pop().unwrap())
  }

  fn resolve_sub_config(
    &'a self,
    config_file_path: PathBuf,
    parent_config: &'a ResolvedConfig,
  ) -> LocalBoxFuture<'a, Result<Vec<PluginsAndPaths<TEnvironment>>>> {
    async move {
      log_verbose!(self.environment, "Analyzing config file {}", config_file_path.display());
      let config_file_path = self.environment.canonicalize(&config_file_path)?;
      let config_path = ResolvedConfigPath {
        base_path: config_file_path.parent().unwrap().to_owned(),
        resolved_path: ResolvedPath::local(config_file_path),
      };
      let mut config = resolve_config_from_path(&config_path, self.environment)?;
      if !self.args.plugins.is_empty() {
        config.plugins = parent_config.plugins.clone();
      }
      let scope = resolve_plugins_scope_and_err_if_empty(&config, self.environment, self.plugin_resolver, self.resolve_plugins_options).await?;
      let glob_output = get_and_resolve_file_paths(&config, self.patterns, &scope.plugins, self.environment).await?;
      let file_paths_by_plugins = get_file_paths_by_plugins_and_err_if_empty(&scope.plugin_name_maps, glob_output.file_paths)?;

      let mut result = vec![PluginsAndPaths { scope, file_paths_by_plugins }];
      for config_file_path in glob_output.config_files {
        result.extend(self.resolve_sub_config(config_file_path, &config).await?);
      }

      Ok(result)
    }
    .boxed_local()
  }
}

pub async fn get_plugins_with_config_from_args<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Arc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  match resolve_config_from_args(args, environment) {
    Ok(config) => {
      resolve_plugins_scope(
        &config,
        environment,
        plugin_resolver,
        &ResolvePluginsOptions {
          // Skip checking these diagnostics when the user provides
          // plugins from the CLI args. They may be doing this to filter
          // to only specific plugins.
          check_top_level_unknown_property_diagnostics: args.plugins.is_empty(),
        },
      )
      .await
    }
    Err(_) => Ok(PluginsScope::default()), // ignore
  }
}

#[derive(Debug, Error)]
#[error("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.")]
pub struct NoPluginsFoundError;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ResolvePluginsError(#[from] anyhow::Error);

pub struct ResolvePluginsOptions {
  pub check_top_level_unknown_property_diagnostics: bool,
}

pub async fn resolve_plugins_scope_and_err_if_empty<TEnvironment: Environment>(
  config: &ResolvedConfig,
  environment: &TEnvironment,
  plugin_resolver: &Arc<PluginResolver<TEnvironment>>,
  options: &ResolvePluginsOptions,
) -> Result<PluginsScope<TEnvironment>> {
  let scope = resolve_plugins_scope(config, environment, plugin_resolver, options).await?;
  if scope.plugins.is_empty() {
    Err(NoPluginsFoundError.into())
  } else {
    Ok(scope)
  }
}

pub async fn resolve_plugins_scope<TEnvironment: Environment>(
  config: &ResolvedConfig,
  environment: &TEnvironment,
  plugin_resolver: &Arc<PluginResolver<TEnvironment>>,
  options: &ResolvePluginsOptions,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  // resolve the plugins
  let plugins = plugin_resolver.resolve_plugins(config.plugins.clone()).await?;
  let mut config_map = config.config_map.clone();

  // resolve each plugin's configuration
  let mut plugins_with_config = Vec::new();
  for plugin in plugins.into_iter() {
    plugins_with_config.push((get_plugin_config_map(&*plugin, &mut config_map)?, plugin));
  }

  // now get global config
  let global_config = get_global_config(
    config_map,
    environment,
    &GetGlobalConfigOptions {
      check_unknown_property_diagnostics: options.check_top_level_unknown_property_diagnostics,
    },
  )?;

  // now set each plugin's config
  let mut plugins = Vec::with_capacity(plugins_with_config.len());
  for (plugin_config, plugin) in plugins_with_config {
    plugins.push(PluginWithConfig {
      plugin,
      associations: plugin_config.associations,
      format_config: Arc::new(FormatConfig {
        id: FormatConfigId::from_raw(1),
        global: global_config.clone(),
        raw: plugin_config.config,
      }),
      has_checked_diagnostics: Default::default(),
      initialized_plugin: Default::default(),
    });
  }

  let plugin_name_maps = PluginNameResolutionMaps::from_plugins(&plugins, &config.base_path)?;
  Ok(PluginsScope {
    environment: environment.clone(),
    plugin_name_maps,
    plugins,
  })
}
