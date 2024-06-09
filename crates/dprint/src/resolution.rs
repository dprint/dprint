use std::cell::RefCell;
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::process::HostFormatCallback;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::PluginInfo;
use indexmap::IndexMap;
use thiserror::Error;

use crate::arg_parser::CliArgs;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::get_global_config;
use crate::configuration::get_plugin_config_map;
use crate::configuration::resolve_config_from_args;
use crate::configuration::resolve_config_from_path;
use crate::configuration::GlobalConfigDiagnostic;
use crate::configuration::ResolveConfigError;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPath;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::paths::get_and_resolve_file_paths;
use crate::paths::get_file_paths_by_plugins;
use crate::paths::FilesPathsByPlugins;
use crate::paths::NoFilesFoundError;
use crate::patterns::FileMatcher;
use crate::plugins::output_plugin_config_diagnostics;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::OutputPluginConfigDiagnosticsError;
use crate::plugins::PluginNameResolutionMaps;
use crate::plugins::PluginResolver;
use crate::plugins::PluginWrapper;
use crate::utils::FastInsecureHasher;
use crate::utils::ResolvedPath;

pub enum GetPluginResult {
  HadDiagnostics(usize),
  Success(InitializedPluginWithConfig),
}

pub struct PluginWithConfig {
  pub plugin: Rc<PluginWrapper>,
  pub associations: Option<Vec<String>>,
  pub format_config: Arc<FormatConfig>,
  pub file_matching: FileMatchingInfo,
  config_diagnostic_count: tokio::sync::Mutex<Option<usize>>,
}

impl PluginWithConfig {
  pub fn new(plugin: Rc<PluginWrapper>, associations: Option<Vec<String>>, format_config: Arc<FormatConfig>, file_matching: FileMatchingInfo) -> Self {
    Self {
      plugin,
      associations,
      format_config,
      config_diagnostic_count: Default::default(),
      file_matching,
    }
  }

  /// Gets a hash that represents the current state of the plugin.
  /// This is used for the "incremental" feature to tell if a plugin has changed state.
  pub fn incremental_hash(&self, hasher: &mut impl Hasher) {
    use std::hash::Hash;
    // list everything in here that would affect formatting
    hasher.write(self.info().name.as_bytes());
    hasher.write(self.info().version.as_bytes());

    // serialize the config keys in order to prevent the hash from changing
    let sorted_config = self.format_config.plugin.iter().collect::<BTreeMap<_, _>>();
    for (key, value) in sorted_config {
      hasher.write(key.as_bytes());
      value.hash(hasher);
    }

    if let Some(associations) = &self.associations {
      for association in associations {
        hasher.write(association.as_bytes());
      }
    }
    self.format_config.global.hash(hasher);
  }

  pub fn name(&self) -> &str {
    &self.info().name
  }

  pub fn info(&self) -> &PluginInfo {
    self.plugin.info()
  }

  pub async fn initialize(self: &Rc<Self>) -> Result<InitializedPluginWithConfig> {
    let instance = self.plugin.initialize().await?;
    Ok(InitializedPluginWithConfig {
      instance,
      plugin: self.clone(),
    })
  }

  pub async fn get_or_create_checking_config_diagnostics<TEnvironment: Environment>(self: &Rc<Self>, environment: &TEnvironment) -> Result<GetPluginResult> {
    // only allow one thread to initialize and output the diagnostics (we don't want the messages being spammed)
    let instance = self.initialize().await?;
    let mut config_diagnostic_count = self.config_diagnostic_count.lock().await;
    match *config_diagnostic_count {
      Some(count) => {
        if count > 0 {
          return Ok(GetPluginResult::HadDiagnostics(count));
        }
        Ok(GetPluginResult::Success(instance))
      }
      None => {
        let result = instance.output_config_diagnostics(environment).await?;
        if let Err(err) = result {
          log_error!(environment, &err.to_string());
          *config_diagnostic_count = Some(err.diagnostic_count);
          Ok(GetPluginResult::HadDiagnostics(err.diagnostic_count))
        } else {
          *config_diagnostic_count = Some(0);
          Ok(GetPluginResult::Success(instance))
        }
      }
    }
  }
}

pub struct InitializedPluginWithConfigFormatRequest {
  pub file_path: PathBuf,
  pub file_bytes: Vec<u8>,
  pub range: FormatRange,
  pub override_config: ConfigKeyMap,
  pub on_host_format: HostFormatCallback,
  pub token: Arc<dyn CancellationToken>,
}

#[derive(Clone)]
pub struct InitializedPluginWithConfig {
  plugin: Rc<PluginWithConfig>,
  instance: Rc<dyn InitializedPlugin>,
}

impl InitializedPluginWithConfig {
  pub fn info(&self) -> &PluginInfo {
    self.plugin.info()
  }

  pub async fn resolved_config(&self) -> Result<String> {
    self.instance.resolved_config(self.plugin.format_config.clone()).await
  }

  pub async fn file_matching_info(&self) -> Result<FileMatchingInfo> {
    self.instance.file_matching_info(self.plugin.format_config.clone()).await
  }

  pub async fn license_text(&self) -> Result<String> {
    self.instance.license_text().await
  }

  pub async fn output_config_diagnostics<TEnvironment: Environment>(
    &self,
    environment: &TEnvironment,
  ) -> Result<Result<(), OutputPluginConfigDiagnosticsError>> {
    output_plugin_config_diagnostics(&self.info().name, &*self.instance, self.plugin.format_config.clone(), environment).await
  }

  pub async fn check_config_updates(&self, plugin_config: ConfigKeyMap) -> Result<Vec<ConfigChange>> {
    self.instance.check_config_updates(plugin_config).await
  }

  pub async fn format_text(&self, request: InitializedPluginWithConfigFormatRequest) -> FormatResult {
    self
      .instance
      .format_text(InitializedPluginFormatRequest {
        file_path: request.file_path,
        file_text: request.file_bytes,
        range: request.range,
        config: self.plugin.format_config.clone(),
        override_config: request.override_config,
        on_host_format: request.on_host_format,
        token: request.token,
      })
      .await
  }
}

pub struct PluginsScope<TEnvironment: Environment> {
  environment: TEnvironment,
  pub config: Option<Rc<ResolvedConfig>>,
  pub plugins: IndexMap<String, Rc<PluginWithConfig>>,
  pub plugin_name_maps: PluginNameResolutionMaps,
  global_config_diagnostics: Vec<GlobalConfigDiagnostic>,
  cached_editor_file_matcher: RefCell<Option<FileMatcher<TEnvironment>>>,
}

impl<TEnvironment: Environment> PluginsScope<TEnvironment> {
  pub fn new(
    environment: TEnvironment,
    plugins: Vec<Rc<PluginWithConfig>>,
    config: Rc<ResolvedConfig>,
    global_config_diagnostics: Vec<GlobalConfigDiagnostic>,
  ) -> Result<Self> {
    let plugin_name_maps = PluginNameResolutionMaps::from_plugins(plugins.iter().map(|p| p.as_ref()), &config.base_path)?;

    Ok(PluginsScope {
      environment,
      config: Some(config),
      plugin_name_maps,
      plugins: plugins.into_iter().map(|p| (p.name().to_string(), p)).collect(),
      global_config_diagnostics,
      cached_editor_file_matcher: Default::default(),
    })
  }

  pub fn ensure_valid_for_cli_args(&self, cli_args: &CliArgs) -> Result<()> {
    self.ensure_no_global_config_diagnostics()?;
    self.ensure_plugins_found()?;
    // Skip checking these diagnostics when the user provides
    // plugins from the CLI args. They may be doing this to filter
    // to only specific plugins.
    if cli_args.plugins.is_empty() {
      self.ensure_no_unknown_config_property_diagnostics()?;
    }
    Ok(())
  }

  pub fn ensure_plugins_found(&self) -> Result<(), NoPluginsFoundError> {
    if self.plugins.is_empty() {
      Err(NoPluginsFoundError)
    } else {
      Ok(())
    }
  }

  pub fn ensure_no_global_config_diagnostics(&self) -> Result<(), ResolveConfigError> {
    if self.global_config_diagnostics.is_empty() {
      return Ok(());
    }
    let diagnostics = self
      .global_config_diagnostics
      .iter()
      .filter_map(|d| match d {
        GlobalConfigDiagnostic::UnknownProperty(_) => None,
        GlobalConfigDiagnostic::Other(d) => Some(d.to_string()),
      })
      .collect::<Vec<_>>();
    self.error_for_diagnostics(&diagnostics)
  }

  pub fn ensure_no_unknown_config_property_diagnostics(&self) -> Result<(), ResolveConfigError> {
    if self.global_config_diagnostics.is_empty() {
      return Ok(());
    }
    let diagnostics = self
      .global_config_diagnostics
      .iter()
      .filter_map(|d| match d {
        GlobalConfigDiagnostic::UnknownProperty(d) => Some(d.to_string()),
        GlobalConfigDiagnostic::Other(_) => None,
      })
      .collect::<Vec<_>>();
    self.error_for_diagnostics(&diagnostics)
  }

  fn error_for_diagnostics(&self, diagnostics: &[String]) -> Result<(), ResolveConfigError> {
    if diagnostics.is_empty() {
      return Ok(());
    }
    let diagnostics_len = diagnostics.len();
    let mut output_text = String::new();
    for diagnostic in diagnostics {
      output_text.push_str("* ");
      output_text.push_str(diagnostic);
      output_text.push('\n');
    }
    output_text.push_str(&format!("\nHad {} config diagnostic(s)", diagnostics_len));
    if let Some(config) = &self.config {
      output_text.push_str(&format!(" in {}", config.resolved_path.source));
    }
    Err(ResolveConfigError::Other(anyhow::anyhow!("{}", output_text)))
  }

  pub fn process_plugin_count(&self) -> usize {
    self.plugins.values().filter(|p| p.plugin.is_process_plugin()).count()
  }

  pub fn get_plugin(&self, name: &str) -> Rc<PluginWithConfig> {
    self
      .plugins
      .get(name)
      .cloned()
      .unwrap_or_else(|| panic!("Expected to find plugin in collection: {}", name))
  }

  pub fn plugins_hash(&self) -> u64 {
    let mut hasher = FastInsecureHasher::default();
    for plugin in self.plugins.values() {
      plugin.incremental_hash(&mut hasher);
    }
    hasher.finish()
  }

  pub fn create_host_format_callback(self: &Rc<Self>) -> HostFormatCallback {
    let scope = self.clone();
    Rc::new(move |host_request| scope.format(host_request))
  }

  pub fn can_format_for_editor(&self, file_path: &Path) -> bool {
    let mut file_matcher_borrow = self.cached_editor_file_matcher.borrow_mut();
    if file_matcher_borrow.is_none() {
      let Some(config) = &self.config else {
        return false;
      };
      let matcher = match FileMatcher::new(self.environment.clone(), config, &FilePatternArgs::default(), &config.base_path) {
        Ok(matcher) => matcher,
        Err(err) => {
          log_warn!(self.environment, "Error creating file matcher: {}", err);
          return false;
        }
      };
      file_matcher_borrow.replace(matcher);
    }
    match file_matcher_borrow.as_mut() {
      Some(file_matcher) => file_matcher.matches_and_dir_not_ignored(file_path),
      None => false, // should never happen
    }
  }

  pub fn format(self: &Rc<Self>, request: HostFormatRequest) -> LocalBoxFuture<'static, FormatResult> {
    let plugin_names = self.plugin_name_maps.get_plugin_names_from_file_path(&request.file_path);
    log_debug!(
      self.environment,
      "Host formatting {} - File length: {} - Plugins: [{}] - Range: {:?}",
      request.file_path.display(),
      request.file_bytes.len(),
      plugin_names.join(", "),
      request.range,
    );
    let scope = self.clone();
    async move {
      let mut file_text = request.file_bytes;
      let mut had_change = false;
      for plugin_name in plugin_names {
        let plugin = scope.get_plugin(&plugin_name);
        match plugin.get_or_create_checking_config_diagnostics(&scope.environment).await {
          Ok(GetPluginResult::Success(initialized_plugin)) => {
            let result = initialized_plugin
              .format_text(InitializedPluginWithConfigFormatRequest {
                file_path: request.file_path.clone(),
                file_bytes: file_text.clone(),
                range: request.range.clone(),
                override_config: request.override_config.clone(),
                on_host_format: scope.create_host_format_callback(),
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
    .boxed_local()
  }
}

pub struct PluginsScopeAndPathsCollection<TEnvironment: Environment> {
  environment: TEnvironment,
  inner: Vec<PluginsScopeAndPaths<TEnvironment>>,
}

impl<TEnvironment: Environment> PluginsScopeAndPathsCollection<TEnvironment> {
  pub fn ensure_valid_for_cli_args(&self, cli_args: &CliArgs) -> Result<()> {
    for scope in &self.inner {
      scope.scope.ensure_valid_for_cli_args(cli_args)?;
    }

    // ensure we found some files
    if !cli_args.sub_command.allow_no_files() {
      let has_cli_file_patterns = cli_args.sub_command.file_patterns().map(|p| !p.include_patterns.is_empty()).unwrap_or(false);
      // when the user specifies a pattern on the command line, just ensure that one scope matched
      if has_cli_file_patterns {
        let all_empty = self.iter().all(|s| s.file_paths_by_plugins.is_empty());
        if all_empty {
          return Err(
            NoFilesFoundError {
              base_path: self.environment.cwd(),
            }
            .into(),
          );
        }
      } else {
        // if no args specified then ensure all scopes have files
        for scope in &self.inner {
          if let Some(config) = scope.scope.config.as_ref() {
            scope.file_paths_by_plugins.ensure_not_empty(&config.base_path)?;
          }
        }
      }
    }

    Ok(())
  }

  pub fn iter(&self) -> impl Iterator<Item = &PluginsScopeAndPaths<TEnvironment>> {
    self.inner.iter()
  }

  pub fn into_iter(self) -> impl Iterator<Item = PluginsScopeAndPaths<TEnvironment>> {
    self.inner.into_iter()
  }
}

pub struct PluginsScopeAndPaths<TEnvironment: Environment> {
  pub scope: PluginsScope<TEnvironment>,
  pub file_paths_by_plugins: FilesPathsByPlugins,
}

pub async fn resolve_plugins_scope_and_paths<TEnvironment: Environment>(
  args: &CliArgs,
  patterns: &FilePatternArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScopeAndPathsCollection<TEnvironment>> {
  let resolver = PluginsAndPathsResolver {
    args,
    patterns,
    environment,
    plugin_resolver,
  };

  resolver.resolve_for_config().await
}

struct PluginsAndPathsResolver<'a, TEnvironment: Environment> {
  args: &'a CliArgs,
  patterns: &'a FilePatternArgs,
  environment: &'a TEnvironment,
  plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>,
}

impl<'a, TEnvironment: Environment> PluginsAndPathsResolver<'a, TEnvironment> {
  pub async fn resolve_for_config(&self) -> Result<PluginsScopeAndPathsCollection<TEnvironment>> {
    let config = Rc::new(resolve_config_from_args(self.args, self.environment).await?);
    let scope = resolve_plugins_scope(config.clone(), self.environment, self.plugin_resolver).await?;
    let glob_output = get_and_resolve_file_paths(&config, self.patterns, scope.plugins.values().map(|p| p.as_ref()), self.environment).await?;
    let file_paths_by_plugins = get_file_paths_by_plugins(&scope.plugin_name_maps, glob_output.file_paths)?;

    let mut result = vec![PluginsScopeAndPaths { scope, file_paths_by_plugins }];
    let root_config_path = config.resolved_path.source.maybe_local_path();
    // todo: parallelize?
    for config_file_path in glob_output.config_files {
      result.extend(self.resolve_for_sub_config(config_file_path, &config, root_config_path).await?);
    }

    Ok(PluginsScopeAndPathsCollection {
      environment: self.environment.clone(),
      inner: result,
    })
  }

  fn resolve_for_sub_config(
    &'a self,
    config_file_path: PathBuf,
    parent_config: &'a ResolvedConfig,
    root_config_path: Option<&'a CanonicalizedPathBuf>,
  ) -> LocalBoxFuture<'a, Result<Vec<PluginsScopeAndPaths<TEnvironment>>>> {
    async move {
      log_debug!(self.environment, "Analyzing config file {}", config_file_path.display());
      let config_file_path = self.environment.canonicalize(&config_file_path)?;
      if Some(&config_file_path) == root_config_path {
        // config file specified via `--config` so ignore it
        return Ok(Vec::new());
      }
      let config_path = ResolvedConfigPath {
        base_path: config_file_path.parent().unwrap(),
        resolved_path: ResolvedPath::local(config_file_path),
      };
      let mut config = resolve_config_from_path(&config_path, self.environment).await?;
      if !self.args.plugins.is_empty() {
        config.plugins.clone_from(&parent_config.plugins);
      }
      let config = Rc::new(config);
      let scope = resolve_plugins_scope(config.clone(), self.environment, self.plugin_resolver).await?;
      let glob_output = get_and_resolve_file_paths(&config, self.patterns, scope.plugins.values().map(|p| p.as_ref()), self.environment).await?;
      let file_paths_by_plugins = get_file_paths_by_plugins(&scope.plugin_name_maps, glob_output.file_paths)?;

      let mut result = vec![PluginsScopeAndPaths { scope, file_paths_by_plugins }];
      // todo: parallelize?
      for config_file_path in glob_output.config_files {
        result.extend(self.resolve_for_sub_config(config_file_path, &config, root_config_path).await?);
      }

      Ok(result)
    }
    .boxed_local()
  }
}

pub async fn get_plugins_scope_from_args<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  match resolve_config_from_args(args, environment).await {
    Ok(config) => resolve_plugins_scope(Rc::new(config), environment, plugin_resolver).await,
    // ignore
    Err(_) => Ok(PluginsScope {
      environment: environment.clone(),
      config: None,
      plugin_name_maps: Default::default(),
      plugins: Default::default(),
      global_config_diagnostics: Default::default(),
      cached_editor_file_matcher: Default::default(),
    }),
  }
}

#[derive(Debug, Error)]
#[error("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.")]
pub struct NoPluginsFoundError;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ResolvePluginsError(#[from] anyhow::Error);

pub async fn resolve_plugins_scope<TEnvironment: Environment>(
  config: Rc<ResolvedConfig>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  // resolve the plugins
  let plugins = plugin_resolver.resolve_plugins(config.plugins.clone()).await?;
  let mut config_map = config.config_map.clone();

  // resolve each plugin's configuration
  let mut plugins_with_config = Vec::new();
  for plugin in plugins.into_iter() {
    plugins_with_config.push((get_plugin_config_map(&plugin, &mut config_map)?, plugin));
  }

  // now get global config
  let global_config_result = get_global_config(config_map);
  let global_config = global_config_result.config;

  // create the scope
  let plugins = plugins_with_config.into_iter().map(|(plugin_config, plugin)| {
    let global_config = global_config.clone();
    let next_config_id = plugin_resolver.next_config_id();
    async move {
      let instance = plugin.initialize().await?;
      let format_config = Arc::new(FormatConfig {
        id: next_config_id,
        global: global_config,
        plugin: plugin_config.properties,
      });
      let file_matching_info = instance.file_matching_info(format_config.clone()).await?;
      Ok::<_, anyhow::Error>(Rc::new(PluginWithConfig::new(
        plugin,
        plugin_config.associations,
        format_config,
        file_matching_info,
      )))
    }
    .boxed_local()
  });
  let plugin_results = dprint_core::async_runtime::future::join_all(plugins).await;
  let mut plugins = Vec::with_capacity(plugin_results.len());
  for result in plugin_results {
    plugins.push(result?);
  }

  Ok(PluginsScope::new(environment.clone(), plugins, config, global_config_result.diagnostics)?)
}
