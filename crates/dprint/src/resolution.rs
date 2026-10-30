use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use deno_terminal::colors;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::CancellationToken;
use dprint_core::plugins::CheckConfigUpdatesMessage;
use dprint_core::plugins::ConfigChange;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::FileMatchingInfo;
use dprint_core::plugins::FormatConfigId;
use dprint_core::plugins::FormatError;
use dprint_core::plugins::FormatRange;
use dprint_core::plugins::FormatResult;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::HostFormatCallback;
use indexmap::IndexMap;
use thiserror::Error;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::ConfigMap;
use crate::configuration::ConfigMapValue;
use crate::configuration::GlobalConfigDiagnostic;
use crate::configuration::Permissions;
use crate::configuration::ProcessPluginApproval;
use crate::configuration::RawPluginConfigOverride;
use crate::configuration::ResolveConfigError;
use crate::configuration::ResolvedConfig;
use crate::configuration::ResolvedConfigPathWithText;
use crate::configuration::get_default_config_file_in_ancestor_directories;
use crate::configuration::get_global_config;
use crate::configuration::get_plugin_config_map;
use crate::configuration::get_settings_file_path;
use crate::configuration::resolve_config_from_args;
use crate::configuration::resolve_config_from_path_with_ancestors;
use crate::configuration::resolve_global_config_path_and_text_at_path_root;
use crate::configuration::resolve_local_config_file_dirs;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::format::stabilize_format_text;
use crate::paths::FilesPathsByPlugins;
use crate::paths::IncludeScope;
use crate::paths::NoFilesFoundError;
use crate::paths::get_and_resolve_file_paths;
use crate::paths::get_file_paths_by_plugins;
use crate::paths::get_plugin_names_for_file_on_disk;
use crate::patterns::ConfigPatternBases;
use crate::patterns::FileMatcher;
use crate::patterns::FileMatcherOptions;
use crate::patterns::OrderedPatternsMatcher;
use crate::plugins::FormatConfig;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::OutputPluginConfigDiagnosticsError;
use crate::plugins::PluginConfigDiagnostic;
use crate::plugins::PluginNameResolutionMaps;
use crate::plugins::PluginResolver;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginWrapper;
use crate::plugins::find_npm_plugin_local_path;
use crate::plugins::output_plugin_config_diagnostics;
use crate::utils::FastInsecureHasher;
use crate::utils::GlobOutput;
use crate::utils::OutsideBasePath;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::escape_glob_text_for_cli;
use crate::utils::get_sha256_checksum;
use crate::utils::is_negated_glob;
use crate::utils::quote_text;

pub enum GetPluginResult {
  HadDiagnostics(Vec<PluginConfigDiagnostic>),
  Success(InitializedPluginWithConfig),
}

/// The error for formatting with a plugin that has configuration diagnostics.
///
/// The diagnostics are only logged the first time the plugin is used, so this
/// has them for a consumer that reports each failure (ex. the lsp).
#[derive(Debug, Error)]
#[error("Had {} configuration errors.", .diagnostics.len())]
pub struct PluginConfigDiagnosticsError {
  pub diagnostics: Vec<PluginConfigDiagnostic>,
}

pub struct PluginConfigOverride {
  files: Vec<String>,
  properties: ConfigKeyMap,
  config_id: FormatConfigId,
  matcher: OrderedPatternsMatcher,
}

pub struct PluginWithConfig {
  pub plugin: Rc<PluginWrapper>,
  pub associations: Option<Vec<String>>,
  pub overrides: Vec<PluginConfigOverride>,
  pub format_config: Arc<FormatConfig>,
  pub file_matching: FileMatchingInfo,
  /// The plugin's resolved configuration serialized as JSON. This is used by
  /// the incremental hash so that values the plugin derives at resolution time
  /// (ex. the exec plugin's `cacheKeyFiles` hash) invalidate the cache.
  serialized_resolved_config: String,
  /// The configuration diagnostics once checked, which is empty for none.
  config_diagnostics: tokio::sync::Mutex<Option<Vec<PluginConfigDiagnostic>>>,
}

pub struct PluginWithConfigOptions {
  pub associations: Option<Vec<String>>,
  pub format_config: Arc<FormatConfig>,
  pub file_matching: FileMatchingInfo,
  pub overrides: Vec<PluginConfigOverride>,
  /// The plugin's resolved configuration serialized as JSON.
  pub serialized_resolved_config: String,
}

impl PluginWithConfig {
  pub fn new(plugin: Rc<PluginWrapper>, options: PluginWithConfigOptions) -> Self {
    Self {
      plugin,
      associations: options.associations,
      overrides: options.overrides,
      format_config: options.format_config,
      config_diagnostics: Default::default(),
      file_matching: options.file_matching,
      serialized_resolved_config: options.serialized_resolved_config,
    }
  }

  /// Gets a hash that represents the current state of the plugin.
  /// This is used for the "incremental" feature to tell if a plugin has changed state.
  pub fn incremental_hash(&self, hasher: &mut impl Hasher) {
    use std::hash::Hash;
    // list everything in here that would affect formatting, including the
    // length of anything that varies in length so that different states
    // can't feed the same bytes to the hasher
    self.info().name.hash(hasher);
    self.info().version.hash(hasher);

    // serialize the config keys in order to prevent the hash from changing
    let sorted_config = self.format_config.plugin.iter().collect::<BTreeMap<_, _>>();
    sorted_config.len().hash(hasher);
    for (key, value) in sorted_config {
      key.hash(hasher);
      value.hash(hasher);
    }

    // include the plugin's resolved config so that anything it derives at
    // resolution time but isn't present in the raw config map (ex. the exec
    // plugin folding `cacheKeyFiles` contents into its `cacheKey`) busts the cache
    self.serialized_resolved_config.hash(hasher);

    self.associations.is_some().hash(hasher);
    if let Some(associations) = &self.associations {
      associations.len().hash(hasher);
      for association in associations {
        association.hash(hasher);
      }
    }
    self.overrides.len().hash(hasher);
    for override_config in &self.overrides {
      override_config.files.len().hash(hasher);
      for file in &override_config.files {
        file.hash(hasher);
      }
      let sorted_config = override_config.properties.iter().collect::<BTreeMap<_, _>>();
      sorted_config.len().hash(hasher);
      for (key, value) in sorted_config {
        key.hash(hasher);
        value.hash(hasher);
      }
    }
    self.format_config.global.hash(hasher);
  }

  pub fn get_config_file_overrides_for_path(&self, file_path: &Path) -> ConfigKeyMap {
    let mut result = ConfigKeyMap::new();
    for override_config in &self.overrides {
      if override_config.matcher.matches(file_path) {
        for (key, value) in override_config.properties.iter() {
          result.insert(key.clone(), value.clone());
        }
      }
    }
    result
  }

  pub fn get_merged_overrides_for_path(&self, file_path: &Path, request_override_config: &ConfigKeyMap, is_plugin_request: bool) -> ConfigKeyMap {
    let mut result = self.get_config_file_overrides_for_path(file_path);
    // IMPORTANT: A plugin's request can come from any plugin (ex. a Wasm plugin
    // from remote configuration formatting a code block), so its override config
    // may only change the global configuration of a process plugin. Process plugins
    // don't run sandboxed, so it could otherwise get them to do something like run
    // commands of its choosing (ex. dprint-plugin-exec).
    let only_global_config = is_plugin_request && self.plugin.is_process_plugin();
    for (key, value) in request_override_config.iter() {
      if only_global_config && !GLOBAL_CONFIG_KEYS.contains(&key.as_str()) {
        continue;
      }
      result.insert(key.clone(), value.clone());
    }
    result
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
    let mut config_diagnostics = self.config_diagnostics.lock().await;
    match &*config_diagnostics {
      Some(diagnostics) => {
        if !diagnostics.is_empty() {
          return Ok(GetPluginResult::HadDiagnostics(diagnostics.clone()));
        }
        Ok(GetPluginResult::Success(instance))
      }
      None => {
        let result = instance.output_config_diagnostics(environment).await?;
        if let Err(err) = result {
          log_error!(environment, &err.to_string());
          *config_diagnostics = Some(err.diagnostics.clone());
          Ok(GetPluginResult::HadDiagnostics(err.diagnostics))
        } else {
          let result = instance.output_override_config_diagnostics(environment).await?;
          if let Err(err) = result {
            log_error!(environment, &err.to_string());
            *config_diagnostics = Some(err.diagnostics.clone());
            Ok(GetPluginResult::HadDiagnostics(err.diagnostics))
          } else {
            *config_diagnostics = Some(Vec::new());
            Ok(GetPluginResult::Success(instance))
          }
        }
      }
    }
  }
}

pub struct InitializedPluginWithConfigFormatRequest {
  pub file_path: PathBuf,
  pub file_bytes: Arc<Vec<u8>>,
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

  pub async fn output_override_config_diagnostics<TEnvironment: Environment>(
    &self,
    environment: &TEnvironment,
  ) -> Result<Result<(), OutputPluginConfigDiagnosticsError>> {
    let mut diagnostics = Vec::new();
    for override_config in &self.plugin.overrides {
      let mut plugin_config = self.plugin.format_config.plugin.clone();
      for (key, value) in override_config.properties.iter() {
        plugin_config.insert(key.clone(), value.clone());
      }
      let format_config = Arc::new(FormatConfig {
        id: override_config.config_id,
        plugin: plugin_config,
        global: self.plugin.format_config.global.clone(),
      });
      for diagnostic in self.instance.config_diagnostics(format_config).await? {
        let diagnostic = PluginConfigDiagnostic {
          plugin_name: self.info().name.to_string(),
          diagnostic,
        };
        log_warn!(environment, "{}", diagnostic);
        diagnostics.push(diagnostic);
      }
    }

    if !diagnostics.is_empty() {
      Ok(Err(OutputPluginConfigDiagnosticsError {
        plugin_name: self.info().name.to_string(),
        diagnostic_count: diagnostics.len(),
        diagnostics,
      }))
    } else {
      Ok(Ok(()))
    }
  }

  pub async fn check_config_updates(&self, message: CheckConfigUpdatesMessage) -> Result<Vec<ConfigChange>> {
    self.instance.check_config_updates(message).await
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

/// Whether the plugins that format a file format the range of a format
/// request, which is only `Supported` when every one of them says it is and
/// `NotSupported` when any of them says it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeFormattingSupport {
  Supported,
  NotSupported,
  /// A plugin doesn't say, so it's given the range's whole lines.
  Unknown,
}

impl RangeFormattingSupport {
  fn from_plugins(plugin_support: impl Iterator<Item = Option<bool>>) -> Self {
    let mut result = RangeFormattingSupport::Supported;
    for supports in plugin_support {
      match supports {
        Some(true) => {}
        Some(false) => return RangeFormattingSupport::NotSupported,
        None => result = RangeFormattingSupport::Unknown,
      }
    }
    result
  }
}

pub struct PluginsScope<TEnvironment: Environment> {
  environment: TEnvironment,
  pub config: Option<Rc<ResolvedConfig>>,
  pub plugins: IndexMap<String, Rc<PluginWithConfig>>,
  pub plugin_name_maps: PluginNameResolutionMaps,
  /// The urls the plugins need to be downloaded from where the download policy
  /// doesn't allow without asking the user (ex. a domain that isn't trusted),
  /// which has the scope not have any plugins.
  pub untrusted_downloads: Vec<String>,
  /// The process plugins the user didn't approve running, which has the scope
  /// not have any plugins so that the editors don't format with the config
  /// file until they're approved.
  pub unapproved_process_plugins: Option<UnapprovedProcessPlugins>,
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
    let plugin_name_maps = PluginNameResolutionMaps::from_plugins(plugins.iter().map(|p| p.as_ref()), config.pattern_bases(), config.shebangs.as_ref())?;

    Ok(PluginsScope {
      environment,
      config: Some(config),
      plugin_name_maps,
      plugins: plugins.into_iter().map(|p| (p.name().to_string(), p)).collect(),
      untrusted_downloads: Vec::new(),
      unapproved_process_plugins: None,
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
    if self.plugins.is_empty() { Err(NoPluginsFoundError) } else { Ok(()) }
  }

  /// Gets whether the plugins that format the file format the range of a
  /// format request (see `PluginInfo::supports_range_formatting`).
  pub fn range_formatting_support(&self, file_path: &Path, file_bytes: &[u8]) -> RangeFormattingSupport {
    let plugin_names = self.plugin_name_maps.get_plugin_names_from_file_path_and_bytes(file_path, file_bytes);
    RangeFormattingSupport::from_plugins(
      plugin_names
        .iter()
        .map(|name| self.plugins.get(name).and_then(|plugin| plugin.info().supports_range_formatting)),
    )
  }

  /// The problems with the global configuration (ex. an unknown property).
  pub fn global_config_diagnostics(&self) -> &[GlobalConfigDiagnostic] {
    &self.global_config_diagnostics
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
      output_text.push_str(&format!(" in {}", config.source));
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
    use std::hash::Hash;
    let mut hasher = FastInsecureHasher::default();
    self.plugins.len().hash(&mut hasher);
    for plugin in self.plugins.values() {
      plugin.incremental_hash(&mut hasher);
    }
    // the shebang mappings affect which plugin formats a file
    if let Some(shebangs) = self.config.as_ref().and_then(|c| c.shebangs.as_ref()) {
      shebangs.len().hash(&mut hasher);
      for (shebang, extension) in shebangs {
        shebang.hash(&mut hasher);
        extension.hash(&mut hasher);
      }
      // version bump in order to format the files of configs with shebangs
      // again because we changed how files matched by a shebang get formatted
      1u8.hash(&mut hasher);
    }
    hasher.finish()
  }

  pub fn create_host_format_callback(self: &Rc<Self>) -> HostFormatCallback {
    let scope = self.clone();
    Rc::new(move |host_request| scope.format_inner(host_request, /* is plugin request */ true))
  }

  /// Whether the editor should ask this scope to format the file.
  ///
  /// `file_bytes_start` is the start of the editor's in-memory text when it has
  /// it (ex. the LSP), which is what an extensionless file's shebang needs to be
  /// resolved from so an unsaved shebang still matches. When it's `None` (ex. the
  /// editor service, which only receives a path) the file on disk is used.
  pub fn can_format_for_editor(&self, file_path: &Path, file_bytes_start: Option<&[u8]>) -> bool {
    if !self.matches_editor_file_patterns(file_path) {
      return false;
    }

    // Extensionless files match the includes patterns whenever any shebangs are
    // configured because their shebang isn't known up front, so ensure one
    // actually resolves to a plugin rather than claiming every extensionless file.
    if self.plugin_name_maps.may_match_shebang(file_path) {
      return match file_bytes_start {
        Some(file_bytes_start) => !self
          .plugin_name_maps
          .get_plugin_names_from_file_path_and_bytes(file_path, file_bytes_start)
          .is_empty(),
        None => !get_plugin_names_for_file_on_disk(&self.plugin_name_maps, file_path, &self.environment).is_empty(),
      };
    }

    true
  }

  /// Gets whether the config file's includes and excludes (and the
  /// gitignore files) match the file, which doesn't need the plugins.
  pub fn matches_editor_file_patterns(&self, file_path: &Path) -> bool {
    let mut file_matcher_borrow = self.cached_editor_file_matcher.borrow_mut();
    if file_matcher_borrow.is_none() {
      let Some(config) = &self.config else {
        return false;
      };
      let matcher = match FileMatcher::new(
        self.environment.clone(),
        FileMatcherOptions {
          config,
          args: &FilePatternArgs::default(),
          root_dir: &config.base_path,
          specified_file_path: None,
          // this matcher lives as long as the scope, which in an editor is until
          // the dprint config changes, and gitignores change more often than that
          detect_gitignore_changes: true,
        },
      ) {
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
    self.format_inner(request, /* is plugin request */ false)
  }

  fn format_inner(self: &Rc<Self>, request: HostFormatRequest, is_plugin_request: bool) -> LocalBoxFuture<'static, FormatResult> {
    let scope = self.clone();
    async move {
      let result = scope
        .format_shared(SharedFormatRequest {
          file_path: request.file_path,
          file_bytes: Arc::new(request.file_bytes),
          range: request.range,
          override_config: request.override_config,
          is_plugin_request,
          token: request.token,
        })
        .await?;
      Ok(result.map(Arc::unwrap_or_clone))
    }
    .boxed_local()
  }

  /// Formats the text like `format`, then formats a changed result again until
  /// it's stable the same way `dprint fmt` does. This shouldn't be used for host
  /// formatting (ex. code blocks) because the outer text is already made stable.
  pub async fn format_stable(self: &Rc<Self>, request: HostFormatRequest, ensure_stable_format: EnsureStableFormat) -> FormatResult {
    // the range wouldn't apply to the formatted text, so only format a range once
    if !ensure_stable_format.0 || request.range.is_some() {
      return self.format(request).await;
    }
    let file_path = request.file_path;
    let override_config = request.override_config;
    let token = request.token;
    let new_request = |file_bytes: Arc<Vec<u8>>| SharedFormatRequest {
      file_path: file_path.clone(),
      file_bytes,
      range: None,
      override_config: override_config.clone(),
      is_plugin_request: false,
      token: token.clone(),
    };
    let original_text = Arc::new(request.file_bytes);
    let Some(formatted_text) = self.clone().format_shared(new_request(original_text.clone())).await? else {
      return Ok(None);
    };
    // a plugin may say it changed the text without changing it
    if formatted_text == original_text {
      return Ok(Some(Arc::unwrap_or_clone(formatted_text)));
    }
    let stable_text = stabilize_format_text(&self.environment, &file_path, formatted_text, |text| {
      let scope = self.clone();
      let request = new_request(text.clone());
      async move {
        // the result gets discarded, so stop by saying the text didn't change
        if request.token.is_cancelled() {
          return Ok(text);
        }
        match scope.format_shared(request).await {
          Ok(new_text) => Ok(new_text.unwrap_or(text)),
          Err(err) => Err(anyhow::anyhow!(dprint_core::plugins::error_to_string(&err))),
        }
      }
    })
    .await
    .map_err(FormatError::new)?;
    Ok(Some(Arc::unwrap_or_clone(stable_text)))
  }

  /// Formats text that's shared with the plugins instead of copied for each one.
  async fn format_shared(self: Rc<Self>, request: SharedFormatRequest) -> Result<Option<Arc<Vec<u8>>>, FormatError> {
    let SharedFormatRequest {
      file_path,
      file_bytes,
      range,
      override_config,
      is_plugin_request,
      token,
    } = request;
    let plugins = self
      .plugin_name_maps
      .get_plugin_names_from_file_path_and_bytes(&file_path, &file_bytes)
      .iter()
      .map(|plugin_name| self.get_plugin(plugin_name))
      .collect::<Vec<_>>();
    log_debug!(
      self.environment,
      "Host formatting {} - File length: {} - Plugins: [{}] - Range: {:?}",
      file_path.display(),
      file_bytes.len(),
      plugins.iter().map(|plugin| plugin.name()).collect::<Vec<_>>().join(", "),
      range,
    );
    let plugin_file_path = self.plugin_name_maps.get_plugin_file_path(&file_path, &file_bytes);
    // a plugin might be asking to format with the file path it was given
    let real_file_path = self.plugin_name_maps.get_real_file_path(&file_path);
    let mut file_text = file_bytes;
    let mut had_change = false;
    for plugin in plugins {
      match plugin.get_or_create_checking_config_diagnostics(&self.environment).await {
        Ok(GetPluginResult::Success(initialized_plugin)) => {
          let result = initialized_plugin
            .format_text(InitializedPluginWithConfigFormatRequest {
              file_path: plugin_file_path.to_path_buf(),
              file_bytes: file_text.clone(),
              range: range.clone(),
              override_config: plugin.get_merged_overrides_for_path(real_file_path, &override_config, is_plugin_request),
              on_host_format: self.create_host_format_callback(),
              token: token.clone(),
            })
            .await;
          if let Some(new_text) = result? {
            file_text = Arc::new(new_text);
            had_change = true;
          }
        }
        Ok(GetPluginResult::HadDiagnostics(diagnostics)) => return Err(FormatError::new(PluginConfigDiagnosticsError { diagnostics })),
        Err(err) => return Err(CriticalFormatError(FormatError::new(err)).into()),
      }
    }

    Ok(if had_change { Some(file_text) } else { None })
  }
}

/// A `HostFormatRequest` whose text is shared, so formatting it with several
/// plugins or several times doesn't copy the text.
struct SharedFormatRequest {
  file_path: PathBuf,
  file_bytes: Arc<Vec<u8>>,
  range: FormatRange,
  override_config: ConfigKeyMap,
  /// Whether a plugin made the request (ex. to format a code block) instead of
  /// dprint or an editor.
  is_plugin_request: bool,
  token: Arc<dyn CancellationToken>,
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
      let cli_file_patterns = cli_args.sub_command.file_patterns().and_then(|p| p.include_patterns.as_ref());
      match cli_file_patterns {
        // the user explicitly specified no files to format (ex. `--stdin-files`
        // with no lines), so there's nothing to format and that's ok
        Some(patterns) if patterns.is_empty() => {}
        // when the user specifies a pattern on the command line, just ensure that one scope matched
        Some(_) => {
          let all_empty = self.iter().all(|s| s.file_paths_by_plugins.is_empty());
          if all_empty {
            return Err(
              NoFilesFoundError {
                base_path: self.environment.cwd(),
              }
              .into(),
            );
          }
        }
        // if no args specified then ensure all scopes have files
        None => {
          for scope in &self.inner {
            if let Some(config) = scope.scope.config.as_ref() {
              scope.file_paths_by_plugins.ensure_not_empty(&config.base_path)?;
            }
          }
        }
      }
    }

    Ok(())
  }

  pub fn len(&self) -> usize {
    self.inner.len()
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

pub struct ResolvePluginsScopeAndPathsOptions {
  pub skip_traversal: bool,
}

pub async fn resolve_plugins_scope_and_paths<TEnvironment: Environment>(
  args: &CliArgs,
  patterns: &FilePatternArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  options: ResolvePluginsScopeAndPathsOptions,
) -> Result<PluginsScopeAndPathsCollection<TEnvironment>> {
  let resolver = PluginsAndPathsResolver {
    args,
    patterns,
    environment,
    plugin_resolver,
    skip_traversal: options.skip_traversal,
  };

  resolver.resolve_for_config().await
}

struct PluginsAndPathsResolver<'a, TEnvironment: Environment> {
  args: &'a CliArgs,
  patterns: &'a FilePatternArgs,
  environment: &'a TEnvironment,
  plugin_resolver: &'a Rc<PluginResolver<TEnvironment>>,
  skip_traversal: bool,
}

impl<'a, TEnvironment: Environment> PluginsAndPathsResolver<'a, TEnvironment> {
  pub async fn resolve_for_config(&'a self) -> Result<PluginsScopeAndPathsCollection<TEnvironment>> {
    let config = Rc::new(resolve_config_from_args(self.args, self.environment).await?);
    let scope = resolve_plugins_scope(config.clone(), self.environment, self.plugin_resolver).await?;
    let config_discovery = self.args.config_discovery(self.environment);
    let mut glob_output = if self.skip_traversal {
      GlobOutput::default()
    } else {
      get_and_resolve_file_paths(
        &config,
        self.patterns,
        config_discovery,
        IncludeScope::AllowAncestors,
        scope.plugins.values().map(|p| p.as_ref()),
        self.environment,
      )
      .await?
    };
    let root_config_path = config.source.maybe_local_path().cloned();

    // resolve specified paths that are outside the config's directory
    // against the config file found in their own directory tree or the
    // user's global config file
    let outside_scopes = self
      .resolve_outside_base_paths(&mut glob_output, &config, config_discovery, root_config_path.clone())
      .await?;

    let file_paths_by_plugins = get_file_paths_by_plugins(&scope.plugin_name_maps, glob_output.file_paths, glob_output.shebang_lines, self.environment)?;

    let mut result = vec![PluginsScopeAndPaths { scope, file_paths_by_plugins }];
    // todo: parallelize?
    let patterns = Rc::new(self.patterns.clone());
    for config_file_path in glob_output.config_files {
      result.extend(
        self
          .resolve_for_sub_config(config_file_path, config.clone(), config_discovery, root_config_path.clone(), patterns.clone())
          .await?,
      );
    }
    result.extend(outside_scopes);

    Ok(PluginsScopeAndPathsCollection {
      environment: self.environment.clone(),
      inner: result,
    })
  }

  /// Resolves the scopes for specified paths and patterns that are outside
  /// the config's directory. Each one uses the config file found in its own
  /// directory tree when one exists, otherwise the explicitly specified or
  /// global config file, and it's an error when there's no config file to use.
  async fn resolve_outside_base_paths(
    &'a self,
    glob_output: &mut GlobOutput,
    config: &Rc<ResolvedConfig>,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
  ) -> Result<Vec<PluginsScopeAndPaths<TEnvironment>>> {
    let outside_base_paths = std::mem::take(&mut glob_output.outside_base_paths);
    if outside_base_paths.is_empty() {
      return Ok(Vec::new());
    }

    // group the paths by the config that governs them
    let mut path_groups: IndexMap<OutsideScopeConfigKey, (OutsideScopeConfig, Vec<String>)> = IndexMap::new();
    for outside_path in outside_base_paths {
      let Some(scope_config) = self.resolve_outside_scope_config(&outside_path, config, config_discovery)? else {
        continue; // skipped with a warning
      };
      path_groups
        .entry(scope_config.group_key())
        .or_insert_with(|| (scope_config, Vec::new()))
        .1
        .push(outside_path.include_pattern);
    }

    let mut result = Vec::new();
    for (_, (scope_config, include_patterns)) in path_groups {
      result.extend(
        self
          .resolve_outside_scope(scope_config, include_patterns, config, config_discovery, root_config_path.clone())
          .await?,
      );
    }
    Ok(result)
  }

  /// Resolves the config that governs the provided outside path. Returns
  /// `None` when the path should be skipped (a warning was logged).
  fn resolve_outside_scope_config(
    &self,
    outside_path: &OutsideBasePath,
    config: &ResolvedConfig,
    config_discovery: ConfigDiscovery,
  ) -> Result<Option<OutsideScopeConfig>> {
    let discover_tree_configs = self.args.config.is_none() && config_discovery.traverse_ancestors();
    if discover_tree_configs && let Some(config_path) = get_default_config_file_in_ancestor_directories(self.environment, &outside_path.config_search_dir)? {
      return Ok(Some(OutsideScopeConfig::ConfigFile(config_path)));
    }

    if self.args.config.is_some() || config.is_global {
      // an explicitly specified config file or the global config file
      // governs explicitly specified paths anywhere
      let root_dir = self.canonical_path_root_dir(&outside_path.config_search_dir)?;
      return Ok(Some(OutsideScopeConfig::RebasedCurrentConfig(root_dir)));
    }

    // only fall back to the global config file when config discovery is in
    // its default mode to match main config file resolution
    if matches!(config_discovery, ConfigDiscovery::Default)
      && let Some(config_path) = self.global_config_path_based_at_path_root(&outside_path.config_search_dir)?
    {
      return Ok(Some(OutsideScopeConfig::ConfigFile(config_path)));
    }

    if self.args.sub_command.allow_skipping_paths() {
      log_warn!(
        self.environment,
        "WARNING: Skipping '{}' because no dprint config file was found for it.",
        outside_path.include_pattern,
      );
      Ok(None)
    } else {
      bail!(
        concat!(
          "No dprint config file found for '{}'. The path is outside the config file's directory ",
          "and no dprint config file was found in the path's ancestor directories. Create one there ",
          "or set up a global config file by running `dprint init --global`."
        ),
        outside_path.include_pattern,
      );
    }
  }

  /// Resolves the scope and file paths for a group of outside paths that
  /// share a governing config.
  async fn resolve_outside_scope(
    &'a self,
    scope_config: OutsideScopeConfig,
    mut include_patterns: Vec<String>,
    config: &Rc<ResolvedConfig>,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
  ) -> Result<Vec<PluginsScopeAndPaths<TEnvironment>>> {
    // carry the negated patterns along so exclusions specified on the
    // command line keep applying in the new scope, but only resolve the
    // grouped paths so files matched by the other args don't get formatted
    // a second time
    include_patterns.extend(self.patterns.include_patterns.iter().flatten().filter(|p| is_negated_glob(p)).cloned());
    // the current scope already handles everything in the config's
    // directory (ex. a `dprint fmt ..` arg covers it with `**`), escaping
    // in case the directory path contains glob characters (ex. `[app]`)
    include_patterns.push(format!("!{}/**", escape_glob_text_for_cli(&config.base_path.to_string_lossy())));
    let patterns = Rc::new(FilePatternArgs {
      include_patterns: Some(include_patterns),
      only_staged: false,
      only_dirty: false,
      ..self.patterns.clone()
    });
    match scope_config {
      OutsideScopeConfig::ConfigFile(config_path) => {
        self
          .resolve_for_config_path(config_path, config.clone(), config_discovery, root_config_path, patterns)
          .await
      }
      OutsideScopeConfig::RebasedCurrentConfig(base_path) => {
        // with an explicitly specified config file, don't let other config
        // files take over parts of the scope
        let config_discovery = if self.args.config.is_some() {
          ConfigDiscovery::IgnoreDescendants
        } else {
          config_discovery
        };
        self
          .resolve_for_rebased_config(config, base_path, config_discovery, root_config_path, patterns)
          .await
      }
    }
  }

  /// Resolves the scope and file paths for the provided config with its base
  /// directory changed to the provided path (ex. resolving paths on another
  /// drive against the in-use config file).
  async fn resolve_for_rebased_config(
    &'a self,
    config: &Rc<ResolvedConfig>,
    base_path: CanonicalizedPathBuf,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
    patterns: Rc<FilePatternArgs>,
  ) -> Result<Vec<PluginsScopeAndPaths<TEnvironment>>> {
    let mut rebased_config = (**config).clone();
    rebased_config.rebase_for_outside_paths(base_path, self.args.config.is_some());
    self
      .resolve_scope_and_descendants(Rc::new(rebased_config), config_discovery, root_config_path, patterns)
      .await
  }

  /// Gets the global config file based at the root directory of the provided
  /// path so the path is within the resulting config's directory.
  fn global_config_path_based_at_path_root(&self, path: &Path) -> Result<Option<ResolvedConfigPathWithText>> {
    Ok(resolve_global_config_path_and_text_at_path_root(self.environment, path)?)
  }

  /// Gets the canonicalized root directory of the provided path (ex. the
  /// drive root on Windows).
  fn canonical_path_root_dir(&self, path: &Path) -> Result<CanonicalizedPathBuf> {
    let root_dir = path.ancestors().last().unwrap();
    Ok(self.environment.canonicalize(root_dir)?)
  }

  async fn resolve_for_sub_config(
    &'a self,
    config_file_path: PathBuf,
    parent_config: Rc<ResolvedConfig>,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
    patterns: Rc<FilePatternArgs>,
  ) -> Result<Vec<PluginsScopeAndPaths<TEnvironment>>> {
    log_debug!(self.environment, "Analyzing config file {}", config_file_path.display());
    let dirs = resolve_local_config_file_dirs(self.environment, &config_file_path)?;
    let config_file_path = self.environment.canonicalize(&config_file_path)?;
    if Some(&config_file_path) == root_config_path.as_ref() {
      // config file specified via `--config` so ignore it
      return Ok(Vec::new());
    }
    let config_path = ResolvedConfigPathWithText {
      content: self.environment.read_file(&config_file_path)?,
      base_path: dirs.base_path,
      anchored_patterns_base_path: dirs.anchored_patterns_base_path,
      source: PathSource::new_local(config_file_path),
      is_global_config: false,
      is_first_download: false,
    };
    self
      .resolve_for_config_path(config_path, parent_config, config_discovery, root_config_path, patterns)
      .await
  }

  /// Resolves the scope and file paths for a config file, recursively
  /// resolving any descendant config files found within its directory.
  fn resolve_for_config_path(
    &'a self,
    config_path: ResolvedConfigPathWithText,
    parent_config: Rc<ResolvedConfig>,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
    patterns: Rc<FilePatternArgs>,
  ) -> LocalBoxFuture<'a, Result<Vec<PluginsScopeAndPaths<TEnvironment>>>> {
    async move {
      // a config file that opts into inheriting does so from the config
      // files in its own ancestor directories
      let mut config = resolve_config_from_path_with_ancestors(&config_path, self.args.remote_cache_mode(), self.environment).await?;
      if !self.args.plugins.is_empty() {
        config.plugins.clone_from(&parent_config.plugins);
      }
      self
        .resolve_scope_and_descendants(Rc::new(config), config_discovery, root_config_path, patterns)
        .await
    }
    .boxed_local()
  }

  /// Resolves the plugins scope and file paths for the provided config,
  /// recursively resolving any descendant config files found within its
  /// directory.
  async fn resolve_scope_and_descendants(
    &'a self,
    config: Rc<ResolvedConfig>,
    config_discovery: ConfigDiscovery,
    root_config_path: Option<CanonicalizedPathBuf>,
    patterns: Rc<FilePatternArgs>,
  ) -> Result<Vec<PluginsScopeAndPaths<TEnvironment>>> {
    let scope = resolve_plugins_scope(config.clone(), self.environment, self.plugin_resolver).await?;
    let mut glob_output = get_and_resolve_file_paths(
      &config,
      &patterns,
      config_discovery,
      IncludeScope::ConfigDir,
      scope.plugins.values().map(|p| p.as_ref()),
      self.environment,
    )
    .await?;
    // paths outside this config's directory were already handled when
    // resolving the root scope
    glob_output.outside_base_paths.clear();
    let file_paths_by_plugins = get_file_paths_by_plugins(&scope.plugin_name_maps, glob_output.file_paths, glob_output.shebang_lines, self.environment)?;

    let mut result = vec![PluginsScopeAndPaths { scope, file_paths_by_plugins }];
    // todo: parallelize?
    for config_file_path in glob_output.config_files {
      result.extend(
        self
          .resolve_for_sub_config(config_file_path, config.clone(), config_discovery, root_config_path.clone(), patterns.clone())
          .await?,
      );
    }
    Ok(result)
  }
}

/// The config governing paths that are outside the main config's directory.
enum OutsideScopeConfig {
  /// A config file found for the outside path (in its directory tree or
  /// the user's global config file).
  ConfigFile(ResolvedConfigPathWithText),
  /// The config file already in use (an explicitly specified `--config`
  /// or the global config file), rebased at the outside path's root
  /// directory so the path is within the scope and the config's
  /// unanchored patterns still apply.
  RebasedCurrentConfig(CanonicalizedPathBuf),
}

impl OutsideScopeConfig {
  fn group_key(&self) -> OutsideScopeConfigKey {
    match self {
      Self::ConfigFile(config_path) => OutsideScopeConfigKey::ConfigFile(config_path.base_path.clone()),
      Self::RebasedCurrentConfig(base_path) => OutsideScopeConfigKey::RebasedCurrentConfig(base_path.clone()),
    }
  }
}

/// Identifies the config governing a group of outside paths.
///
/// The base directory alone isn't enough because a config file could live in
/// the same directory the current config gets rebased at (ex. a config file at
/// a drive root), which would otherwise silently resolve some paths against the
/// wrong config. The way the config is resolved rules that out today, but
/// keying on the variant makes it impossible to get wrong.
#[derive(PartialEq, Eq, Hash)]
enum OutsideScopeConfigKey {
  ConfigFile(CanonicalizedPathBuf),
  RebasedCurrentConfig(CanonicalizedPathBuf),
}

pub async fn get_plugins_scope_from_args<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  get_plugins_scope_from_args_with_approval(args, environment, plugin_resolver, &Permissions::allow_all()).await
}

/// Gets the plugins scope like `get_plugins_scope_from_args`, but only with the
/// process plugins that are approved (see `resolve_plugins_scope_with_approval`).
pub async fn get_plugins_scope_from_args_with_approval<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  permissions: &Permissions,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  match resolve_config_from_args(args, environment).await {
    Ok(config) => resolve_plugins_scope_with_approval(Rc::new(config), environment, plugin_resolver, permissions).await,
    // ignore
    Err(_) => Ok(PluginsScope {
      environment: environment.clone(),
      config: None,
      plugin_name_maps: Default::default(),
      plugins: Default::default(),
      untrusted_downloads: Default::default(),
      unapproved_process_plugins: Default::default(),
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
pub struct ResolvePluginsError(#[from] pub anyhow::Error);

pub async fn resolve_plugins_scope<TEnvironment: Environment>(
  config: Rc<ResolvedConfig>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  resolve_plugins_scope_with_approval(config, environment, plugin_resolver, &Permissions::allow_all()).await
}

/// The process plugins of a config file that the user didn't approve running.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnapprovedProcessPlugins {
  pub config_file: String,
  /// The hash of the trusted configuration of the config file, which allowing
  /// a plugin that isn't set up is for (see `get_trusted_config_hash`).
  pub trusted_config_hash: String,
  pub plugins: Vec<UnapprovedProcessPlugin>,
}

/// A process plugin that the user didn't approve running.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnapprovedProcessPlugin {
  /// The plugin as the config file specifies it (ex. its url and checksum).
  pub plugin: String,
  /// The npm registry it's from when it isn't the default one, which is
  /// shown with it because an `.npmrc` (ex. the project's) can make an npm
  /// specifier that looks familiar come from somewhere else.
  pub registry: Option<String>,
  /// The checksum of its file when the config file doesn't pin it (an npm
  /// plugin without a version), which is part of what allowing it approves
  /// before it's set up like it is after (see `get_process_plugin_approval`).
  pub unpinned_file_checksum: Option<String>,
  /// What allowing it approves, which is `None` when it isn't set up because
  /// its configuration isn't known until then.
  pub approval: Option<ProcessPluginApproval>,
}

impl UnapprovedProcessPlugin {
  /// Gets the plugin with the registry it's from when it isn't the default one.
  /// They're quoted because they're from the project (see `quote_text`).
  pub fn display(&self) -> String {
    match &self.registry {
      Some(registry) => format!("{} from {}", quote_text(&self.plugin), quote_text(registry)),
      None => quote_text(&self.plugin),
    }
  }
}

impl UnapprovedProcessPlugins {
  /// Gets whether the user denied running each of the process plugins, which
  /// means they shouldn't be asked about them.
  pub fn is_denied(&self, permissions: &Permissions) -> bool {
    self.plugins.iter().all(|plugin| match &plugin.approval {
      Some(approval) => permissions.is_process_plugin_denied(approval),
      None => permissions.is_process_plugin_denied(&ProcessPluginApproval::any_config(self.config_file.clone(), plugin.plugin.clone())),
    })
  }

  /// Gets whether the user chose to always deny running each of the process
  /// plugins.
  pub fn is_always_denied(&self, permissions: &Permissions) -> bool {
    self.plugins.iter().all(|plugin| match &plugin.approval {
      Some(approval) => permissions.is_process_plugin_always_denied(approval),
      None => permissions.is_process_plugin_always_denied(&ProcessPluginApproval::any_config(self.config_file.clone(), plugin.plugin.clone())),
    })
  }

  /// Gets the message that says dprint isn't formatting with the config file
  /// because its process plugins weren't allowed.
  pub fn message(&self, permissions: &Permissions) -> String {
    let plugins = self.plugins.iter().map(|plugin| plugin.display()).collect::<Vec<_>>().join(", ");
    let was_allowed_with_other_config = self
      .plugins
      .iter()
      .filter_map(|plugin| plugin.approval.as_ref())
      .any(|approval| permissions.was_allowed_with_other_config(approval));
    let reason = if was_allowed_with_other_config {
      format!("the configuration of its process plugins ({}) changed since they were allowed", plugins)
    } else {
      format!("its process plugins ({}) weren't allowed to run", plugins)
    };
    let mut message = format!(
      concat!(
        "dprint isn't formatting files with {} because {}. ",
        "Process plugins aren't sandboxed, so only allow them when you trust this configuration file."
      ),
      quote_text(&self.config_file),
      reason,
    );
    // it may be why the user is asked (ex. they allowed every process plugin in it)
    if let Some(ignored_settings_message) = &permissions.ignored_settings_message {
      message.push(' ');
      message.push_str(ignored_settings_message);
    }
    message
  }
}

/// Resolves the plugins of the config like `resolve_plugins_scope`, but
/// resolves none of them when running one of its process plugins isn't approved
/// for the config file with its configuration, so they're never set up or started.
/// The other plugins are left out too because formatting only some of the
/// files (or formatting them with a lower precedence plugin) would format
/// differently than the CLI. The process plugins of the global config file
/// are always approved because it's the user's own.
pub async fn resolve_plugins_scope_with_approval<TEnvironment: Environment>(
  config: Rc<ResolvedConfig>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  permissions: &Permissions,
) -> Result<PluginsScope<TEnvironment>, ResolvePluginsError> {
  // When a plugin that isn't installed may not be downloaded without asking the
  // user (ex. an editor and a domain that isn't trusted), none of the plugins are
  // resolved because formatting with only some of them would format differently
  // than the CLI.
  let mut untrusted_downloads = Vec::new();
  for download in config.plugins.iter().flat_map(|plugin| plugin_resolver.get_disallowed_downloads(plugin)) {
    if !untrusted_downloads.contains(&download) {
      untrusted_downloads.push(download);
    }
  }
  // IMPORTANT: This needs to be checked before resolving the plugins because
  // setting up a process plugin runs it to get its information.
  let mut plugin_references = if untrusted_downloads.is_empty() { config.plugins.clone() } else { Vec::new() };
  let config_file = config.source.display();
  let trusted_config_hash = get_trusted_config_hash(&config);
  let unapproved_plugins = get_unapproved_process_plugins(&config, plugin_resolver, environment, permissions)
    .into_iter()
    .map(|(plugin, approval)| UnapprovedProcessPlugin {
      plugin: plugin.to_full_string(),
      registry: plugin_resolver.get_npm_registry(&plugin),
      unpinned_file_checksum: get_unpinned_plugin_file_checksum(&plugin, environment),
      approval,
    })
    .collect::<Vec<_>>();
  let mut unapproved_process_plugins = None;
  if !unapproved_plugins.is_empty() {
    log_debug!(
      environment,
      "Not using the plugins of {} because its process plugins weren't approved",
      config_file
    );
    plugin_references.clear();
    unapproved_process_plugins = Some(UnapprovedProcessPlugins {
      config_file,
      trusted_config_hash,
      plugins: unapproved_plugins,
    });
  }

  // resolve the plugins
  let plugins = filter_duplicate_plugin_names(plugin_resolver.resolve_plugins(plugin_references).await?);
  let mut config_map = config.config_map.clone();
  let mut trusted_config_map = config.trusted_config_map.clone();

  // resolve each plugin's configuration
  let mut plugins_with_config = Vec::new();
  for plugin in plugins.into_iter() {
    let plugin_config = get_plugin_config_map(&plugin, &mut config_map)?;
    let trusted_plugin_config = get_plugin_config_map(&plugin, &mut trusted_config_map)?;
    // IMPORTANT: Process plugins don't run sandboxed, so only use the trusted
    // configuration for them (local config files and remote config extended with
    // a checksum). Otherwise, remote configuration could get them to do something
    // like run commands of its choosing (ex. dprint-plugin-exec).
    let plugin_config = if plugin.is_process_plugin() {
      let has_untrusted_config = plugin_config.properties != trusted_plugin_config.properties
        || plugin_config.associations != trusted_plugin_config.associations
        || plugin_config.overrides != trusted_plugin_config.overrides;
      let config_key = &plugin.info().config_key;
      if has_untrusted_config && plugin_resolver.should_warn_untrusted_config(config_key) {
        log_warn!(environment, &get_warn_remote_process_plugin_config_message(config_key));
      }
      trusted_plugin_config // NEVER CHANGE THIS
    } else {
      plugin_config
    };
    plugins_with_config.push((plugin_config, plugin));
  }

  // a shared config may configure more plugins than the ones being used, so remove the leftover
  // plugin config that only an extended config specified so it's not reported as an unknown property
  config_map.retain(|key, _| !config.extended_only_plugin_config_keys.contains(key));
  if !untrusted_downloads.is_empty() || unapproved_process_plugins.is_some() {
    // the plugins weren't resolved or the config keys of the process plugins aren't
    // known without setting them up, so don't report the plugin config that's left
    // as an unknown property
    config_map.retain(|_, value| !matches!(value, ConfigMapValue::PluginConfig(_)));
  }

  // now get global config
  let global_config_result = get_global_config(config_map);
  let global_config = global_config_result.config;
  let config_bases = config.pattern_bases();

  // create the scope
  let plugins = plugins_with_config
    .into_iter()
    .map(|(plugin_config, plugin)| {
      let global_config = global_config.clone();
      let overrides = resolve_plugin_config_overrides(plugin_config.overrides, config_bases, plugin_resolver)?;
      let next_config_id = plugin_resolver.next_config_id();
      Ok(
        async move {
          let instance = plugin.initialize().await?;
          let format_config = Arc::new(FormatConfig {
            id: next_config_id,
            global: global_config,
            plugin: plugin_config.properties,
          });
          let file_matching = instance.file_matching_info(format_config.clone()).await?;
          let serialized_resolved_config = instance.resolved_config(format_config.clone()).await?;
          Ok::<_, anyhow::Error>(Rc::new(PluginWithConfig::new(
            plugin,
            PluginWithConfigOptions {
              associations: plugin_config.associations,
              format_config,
              file_matching,
              overrides,
              serialized_resolved_config,
            },
          )))
        }
        .boxed_local(),
      )
    })
    .collect::<Result<Vec<_>>>()?;
  let plugin_results = dprint_core::async_runtime::future::join_all(plugins).await;
  let mut plugins = Vec::with_capacity(plugin_results.len());
  for result in plugin_results {
    plugins.push(result?);
  }

  let mut scope = PluginsScope::new(environment.clone(), plugins, config, global_config_result.diagnostics)?;
  scope.untrusted_downloads = untrusted_downloads;
  scope.unapproved_process_plugins = unapproved_process_plugins;
  Ok(scope)
}

/// Gets what the user approves by allowing each process plugin of the config
/// file to run, which is `None` for a plugin that isn't set up because its
/// configuration isn't known until then (ex. its config key). It's empty for
/// the global config file because it's the user's own.
pub fn get_process_plugin_approvals<TEnvironment: Environment>(
  config: &ResolvedConfig,
  plugin_resolver: &PluginResolver<TEnvironment>,
  environment: &TEnvironment,
) -> Vec<(PluginSourceReference, Option<ProcessPluginApproval>)> {
  if config.is_global {
    return Vec::new();
  }
  config
    .plugins
    .iter()
    .filter(|plugin| plugin.plugin_kind() != Some(PluginKind::Wasm))
    .map(|plugin| {
      // IMPORTANT: Don't set up the plugin to get its information because that runs it.
      let approval = plugin_resolver
        .get_set_up_plugin_info(plugin)
        .map(|info| get_process_plugin_approval(config, plugin, &info.config_key, environment));
      (plugin.clone(), approval)
    })
    .collect()
}

/// Gets the process plugins of the config file that the permissions don't
/// allow running with their configuration (see `get_process_plugin_approvals`),
/// or setting up when they aren't set up, which is empty when using its
/// plugins is approved.
pub fn get_unapproved_process_plugins<TEnvironment: Environment>(
  config: &ResolvedConfig,
  plugin_resolver: &PluginResolver<TEnvironment>,
  environment: &TEnvironment,
  permissions: &Permissions,
) -> Vec<(PluginSourceReference, Option<ProcessPluginApproval>)> {
  let config_file = config.source.display();
  let trusted_config_hash = get_trusted_config_hash(config);
  get_process_plugin_approvals(config, plugin_resolver, environment)
    .into_iter()
    .filter(|(plugin, approval)| match approval {
      Some(approval) => !permissions.is_process_plugin_allowed(approval),
      // a plugin the user allowed setting up is allowed with its configuration
      // once it's set up, which the editor records after resolving it
      None => !permissions.may_set_up_process_plugin(
        &config_file,
        &plugin.to_full_string(),
        &trusted_config_hash,
        get_unpinned_plugin_file_checksum(plugin, environment).as_deref(),
      ),
    })
    .collect()
}

/// Gets the hash of the trusted configuration of the config file, which is
/// what the user allows a process plugin that isn't set up with because the
/// configuration of the plugin isn't known until then. It starts with the
/// version of how it's made and a colon like the config hash of a process
/// plugin approval (see `TRUSTED_CONFIG_HASH_VERSION`).
pub fn get_trusted_config_hash(config: &ResolvedConfig) -> String {
  get_trusted_config_map_hash(&config.trusted_config_map)
}

/// Gets the hash of a trusted configuration like `get_trusted_config_hash`.
pub(crate) fn get_trusted_config_map_hash(trusted_config_map: &ConfigMap) -> String {
  let text = serde_json::to_string(trusted_config_map).expect("config should serialize");
  format!("{}:{}", TRUSTED_CONFIG_HASH_VERSION, get_sha256_checksum(text.as_bytes()))
}

/// Gets the message that says how to have the process plugins of a config file
/// run when the editor can't ask whether to allow them.
pub fn get_cant_ask_message(environment: &impl Environment, config_file: &str) -> String {
  let file_path = get_settings_file_path(environment).map(|path| path.display().to_string());
  format!(
    concat!(
      "The editor can't ask whether to allow them, so dprint only formats with it once you allow them by running ",
      "{} or when \"editor.allowProcessPlugins\" isn't false in {}."
    ),
    quote_text(&format!("dprint config install --config \"{}\"", config_file)),
    file_path.as_deref().unwrap_or("settings.json in dprint's global config directory"),
  )
}

/// Gets what the user approves when allowing a process plugin of the config
/// file to run, which is the plugin with its configuration so that a change to
/// either (ex. the commands of dprint-plugin-exec) needs to be approved again.
/// The global configuration isn't part of it because it can't have a plugin do
/// something else.
fn get_process_plugin_approval(
  config: &ResolvedConfig,
  plugin: &PluginSourceReference,
  config_key: &str,
  environment: &impl Environment,
) -> ProcessPluginApproval {
  // only the trusted config is used for process plugins
  let config_hash = get_process_plugin_config_hash(
    get_unpinned_plugin_file_checksum(plugin, environment),
    config.trusted_config_map.get(config_key),
  );
  ProcessPluginApproval {
    config_file: config.source.display(),
    plugin: plugin.to_full_string(),
    config_hash: Some(config_hash),
  }
}

/// The version of how the config hash of a process plugin approval is made
/// (see `ProcessPluginApproval::config_hash`), which is stored with it and
/// needs to be increased when what's hashed changes (ex. a property is added
/// to `ApprovedPlugin` or the configuration serializes differently) so that a
/// decision isn't reported as being for another configuration. An allow with
/// a hash made another way isn't used, and storing an allow keeps it, so the
/// versions of dprint that make it differently don't undo each other's allows
/// (see `EditorApprovals`).
pub(crate) const PROCESS_PLUGIN_CONFIG_HASH_VERSION: u32 = 1;

/// The version of how the hash of the trusted configuration of a config file
/// is made (see `get_trusted_config_hash`), which needs to be increased when
/// what's hashed changes (ex. the configuration serializes differently). The
/// hash is part of the approval ids of process plugins that aren't set up, so
/// that changes them too.
const TRUSTED_CONFIG_HASH_VERSION: u32 = 1;

/// Gets the hash of what's approved with a process plugin, which starts with
/// how it's made (see `PROCESS_PLUGIN_CONFIG_HASH_VERSION`).
fn get_process_plugin_config_hash(plugin_file: Option<String>, config: Option<&ConfigMapValue>) -> String {
  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct ApprovedPlugin<'a> {
    /// The checksum of the plugin file when the config file doesn't pin it.
    plugin_file: Option<String>,
    config: Option<&'a ConfigMapValue>,
  }

  let text = serde_json::to_string(&ApprovedPlugin { plugin_file, config }).expect("config should serialize");
  format!("{}:{}", PROCESS_PLUGIN_CONFIG_HASH_VERSION, get_sha256_checksum(text.as_bytes()))
}

/// Gets the checksum of the plugin file when the config file doesn't pin it,
/// which is an npm plugin without a version that's resolved from node_modules
/// (ex. updating the package changes it). Every other process plugin needs a
/// checksum in the config file, and the plugin file pins the executables with
/// their checksums.
pub fn get_unpinned_plugin_file_checksum(plugin: &PluginSourceReference, environment: &impl Environment) -> Option<String> {
  let PathSource::Npm(npm_source) = &plugin.path_source else {
    return None;
  };
  if npm_source.specifier.version.is_some() {
    return None;
  }
  let cwd = environment.cwd();
  let config_dir = npm_source.base_dir.as_ref().map(|dir| dir.as_ref()).unwrap_or(cwd.as_ref());
  let checksum = find_npm_plugin_local_path(&npm_source.specifier, config_dir, environment)
    .ok()
    .and_then(|path| path.maybe_local_path().and_then(|path| environment.read_file_bytes(path).ok()))
    .map(|bytes| get_sha256_checksum(&bytes));
  // resolving the plugin fails later when it can't be read
  Some(checksum.unwrap_or_else(|| "unreadable".to_string()))
}

/// Keeps only the highest precedence plugin for each plugin name.
///
/// The same plugin may end up specified more than once with different sources
/// (ex. a config pinning a newer version of a plugin that the config it extends
/// also specifies). Plugins are ordered by descending precedence, so the first
/// entry for a name wins.
///
/// This can't be done when resolving the configuration because a plugin's name
/// is only known once it has been resolved.
fn filter_duplicate_plugin_names(plugins: Vec<Rc<PluginWrapper>>) -> Vec<Rc<PluginWrapper>> {
  let mut names = HashSet::with_capacity(plugins.len());

  plugins.into_iter().filter(|plugin| names.insert(plugin.info().name.clone())).collect()
}

fn resolve_plugin_config_overrides<TEnvironment: Environment>(
  overrides: Vec<RawPluginConfigOverride>,
  config_bases: ConfigPatternBases,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<Vec<PluginConfigOverride>> {
  overrides
    .into_iter()
    .map(|override_config| {
      let matcher = OrderedPatternsMatcher::new(&override_config.files, config_bases)?;
      Ok(PluginConfigOverride {
        files: override_config.files,
        properties: override_config.properties,
        config_id: plugin_resolver.next_config_id(),
        matcher,
      })
    })
    .collect()
}

/// The keys of the global configuration (ex. `"lineWidth": 80` at the top level
/// of a config file), which is passed to every plugin.
const GLOBAL_CONFIG_KEYS: [&str; 4] = ["lineWidth", "indentWidth", "useTabs", "newLineKind"];

fn get_warn_remote_process_plugin_config_message(config_key: &str) -> String {
  format!(
    "{} The '{}' configuration specified in remote configuration is ignored for security reasons because it's for a process plugin. Specify it in a local configuration file or extend the remote configuration with a checksum (ex. https://example.com/dprint.json@<sha256>) instead.",
    colors::bold("Note: "),
    config_key,
  )
}

#[cfg(test)]
mod test {
  use dprint_core::configuration::ConfigKeyValue;
  use dprint_core::configuration::GlobalConfiguration;

  use crate::plugins::TestPlugin;

  use super::*;

  // a plugin can derive values at resolution time that aren't present in the
  // raw config map (ex. the exec plugin folds the contents of `cacheKeyFiles`
  // into its `cacheKey`). these must participate in the incremental hash so that
  // changing one of those files invalidates the cache. see issue #1135.
  #[test]
  fn incremental_hash_includes_resolved_config() {
    fn hash_with_resolved_config(resolved_config: &str) -> u64 {
      let plugin = Rc::new(PluginWrapper::new(Box::new(TestPlugin::new("test-plugin", "test-plugin", vec!["txt"], vec![]))));
      let format_config = Arc::new(FormatConfig {
        id: FormatConfigId::from_raw(1),
        global: Default::default(),
        plugin: Default::default(),
      });
      let plugin_with_config = PluginWithConfig::new(
        plugin,
        PluginWithConfigOptions {
          associations: None,
          format_config,
          file_matching: FileMatchingInfo {
            file_extensions: vec!["txt".to_string()],
            file_names: vec![],
            additive: false,
          },
          overrides: Vec::new(),
          serialized_resolved_config: resolved_config.to_string(),
        },
      );
      let mut hasher = FastInsecureHasher::default();
      plugin_with_config.incremental_hash(&mut hasher);
      hasher.finish()
    }

    // the same raw config map but a different resolved config must produce a different hash
    assert_ne!(
      hash_with_resolved_config(r#"{"cacheKey":"a"}"#),
      hash_with_resolved_config(r#"{"cacheKey":"b"}"#)
    );
    // and the same resolved config must produce the same hash
    assert_eq!(
      hash_with_resolved_config(r#"{"cacheKey":"a"}"#),
      hash_with_resolved_config(r#"{"cacheKey":"a"}"#)
    );
  }

  #[test]
  fn should_include_shebangs_in_plugins_hash() {
    fn hash_with_shebangs(shebangs: Option<IndexMap<String, String>>) -> u64 {
      let environment = crate::environment::TestEnvironment::new();
      let base_path = CanonicalizedPathBuf::new_for_testing("/");
      let config = Rc::new(ResolvedConfig {
        config_map: Default::default(),
        trusted_config_map: Default::default(),
        extended_only_plugin_config_keys: Default::default(),
        local_dependencies: Default::default(),
        base_path: base_path.clone(),
        anchored_patterns_base_path: None,
        source: PathSource::new_local(base_path.join_panic_relative("dprint.json")),
        is_global: false,
        excludes: None,
        includes: None,
        incremental: None,
        shebangs,
        inherit: None,
        plugins: Vec::new(),
      });
      let scope = PluginsScope::new(environment, vec![Rc::new(create_plugin_with_overrides(Vec::new()))], config, Vec::new()).unwrap();
      scope.plugins_hash()
    }
    fn shebangs(entries: &[(&str, &str)]) -> Option<IndexMap<String, String>> {
      Some(entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    assert_eq!(hash_with_shebangs(None), hash_with_shebangs(None));
    assert_eq!(
      hash_with_shebangs(shebangs(&[("#!/bin/sh", "sh")])),
      hash_with_shebangs(shebangs(&[("#!/bin/sh", "sh")]))
    );
    assert_ne!(hash_with_shebangs(None), hash_with_shebangs(shebangs(&[("#!/bin/sh", "sh")])));
    // changing the mapped extension changes which plugin formats the file
    assert_ne!(
      hash_with_shebangs(shebangs(&[("#!/bin/sh", "sh")])),
      hash_with_shebangs(shebangs(&[("#!/bin/sh", "txt")]))
    );
  }

  #[test]
  fn should_hash_associations_with_boundaries() {
    fn hash_with_associations(associations: Vec<&str>) -> u64 {
      let mut plugin = create_plugin_with_overrides(Vec::new());
      plugin.associations = Some(associations.into_iter().map(ToOwned::to_owned).collect());
      get_plugin_hash(&plugin)
    }

    assert_ne!(hash_with_associations(vec!["ab", "c"]), hash_with_associations(vec!["a", "bc"]));
  }

  #[test]
  fn should_hash_override_file_patterns_and_property_keys_with_boundaries() {
    let plugin_ab_c = create_plugin_with_override(vec!["ab".to_string()], ConfigKeyMap::from([("c".to_string(), ConfigKeyValue::from_bool(true))]));
    let plugin_a_bc = create_plugin_with_override(vec!["a".to_string()], ConfigKeyMap::from([("bc".to_string(), ConfigKeyValue::from_bool(true))]));

    assert_ne!(get_plugin_hash(&plugin_ab_c), get_plugin_hash(&plugin_a_bc));
  }

  #[test]
  fn should_include_config_overrides_in_incremental_hash() {
    let config_base_path = CanonicalizedPathBuf::new_for_testing("/");
    let plugin_without_override = create_plugin_with_overrides(Vec::new());
    let plugin_with_override = create_plugin_with_overrides(vec![PluginConfigOverride {
      files: vec!["**/package.txt".to_string()],
      properties: ConfigKeyMap::from([("ending".to_string(), "package".into())]),
      config_id: FormatConfigId::from_raw(2),
      matcher: OrderedPatternsMatcher::new(&["**/package.txt".to_string()], (&config_base_path).into()).unwrap(),
    }]);

    assert_ne!(get_plugin_hash(&plugin_without_override), get_plugin_hash(&plugin_with_override));
  }

  fn get_plugin_hash(plugin: &PluginWithConfig) -> u64 {
    let mut hasher = FastInsecureHasher::default();
    plugin.incremental_hash(&mut hasher);
    hasher.finish()
  }

  fn create_plugin_with_override(files: Vec<String>, properties: ConfigKeyMap) -> PluginWithConfig {
    let config_base_path = CanonicalizedPathBuf::new_for_testing("/config");
    let matcher = OrderedPatternsMatcher::new(&files, (&config_base_path).into()).unwrap();
    create_plugin_with_overrides(vec![PluginConfigOverride {
      files,
      properties,
      config_id: FormatConfigId::from_raw(2),
      matcher,
    }])
  }

  fn create_plugin_with_overrides(overrides: Vec<PluginConfigOverride>) -> PluginWithConfig {
    PluginWithConfig::new(
      Rc::new(PluginWrapper::new(Box::new(TestPlugin::new("test-plugin", "test-plugin", vec!["txt"], vec![])))),
      PluginWithConfigOptions {
        associations: None,
        format_config: Arc::new(FormatConfig {
          id: FormatConfigId::from_raw(1),
          plugin: ConfigKeyMap::from([("ending".to_string(), "base".into())]),
          global: GlobalConfiguration::default(),
        }),
        file_matching: FileMatchingInfo {
          file_extensions: vec!["txt".to_string()],
          file_names: Vec::new(),
          additive: false,
        },
        overrides,
        serialized_resolved_config: String::new(),
      },
    )
  }

  #[test]
  fn should_get_whether_the_plugins_format_a_range() {
    let get = |plugin_support: &[Option<bool>]| RangeFormattingSupport::from_plugins(plugin_support.iter().copied());
    assert_eq!(get(&[Some(true), Some(true)]), RangeFormattingSupport::Supported);
    assert_eq!(get(&[Some(true), None]), RangeFormattingSupport::Unknown);
    assert_eq!(get(&[None, Some(false), Some(true)]), RangeFormattingSupport::NotSupported);
  }

  #[test]
  fn should_not_change_the_config_hash_of_a_process_plugin() {
    use crate::configuration::RawPluginConfig;
    use crate::configuration::RawPluginConfigOverride;
    use dprint_core::configuration::ConfigKeyMap;

    // IMPORTANT: The hashes are stored with what the user allowed, so increase
    // PROCESS_PLUGIN_CONFIG_HASH_VERSION when these change.
    let config = ConfigMapValue::PluginConfig(RawPluginConfig {
      associations: Some(vec!["**/*.txt".to_string()]),
      locked: true,
      overrides: vec![RawPluginConfigOverride {
        files: vec!["sub/**".to_string()],
        properties: ConfigKeyMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(80))]),
      }],
      properties: ConfigKeyMap::from([
        (
          "commands".to_string(),
          ConfigKeyValue::Array(vec![ConfigKeyValue::Object(ConfigKeyMap::from([
            ("command".to_string(), ConfigKeyValue::from_str("fmt")),
            ("exts".to_string(), ConfigKeyValue::Array(vec![ConfigKeyValue::from_str("txt")])),
          ]))]),
        ),
        ("useTabs".to_string(), ConfigKeyValue::from_bool(false)),
        ("other".to_string(), ConfigKeyValue::Null),
      ]),
    });
    assert_eq!(
      get_process_plugin_config_hash(Some("checksum".to_string()), Some(&config)),
      "1:7fb2ae9589f5c2f7e6bde8c7a085905d67ded6856bd76721405273d002d6931f"
    );
    assert_eq!(
      get_process_plugin_config_hash(None, None),
      "1:b7c29ac8d0482200b6bcc038d421169c8604a546b3fca5b95b5baf3dfb4c1432"
    );
  }

  #[test]
  fn should_not_change_the_trusted_config_hash() {
    use crate::configuration::RawPluginConfig;
    use dprint_core::configuration::ConfigKeyMap;

    // IMPORTANT: The hash is part of the approval ids that the editor extensions
    // send a decision again for after the server restarts, so increase
    // TRUSTED_CONFIG_HASH_VERSION when these change.
    let config_map = ConfigMap::from([
      ("lineWidth".to_string(), ConfigMapValue::from_i32(80)),
      ("useTabs".to_string(), ConfigMapValue::from_bool(true)),
      ("excludes".to_string(), ConfigMapValue::Vec(vec!["**/target".to_string()])),
      (
        "exec".to_string(),
        ConfigMapValue::PluginConfig(RawPluginConfig {
          associations: None,
          locked: false,
          overrides: Vec::new(),
          properties: ConfigKeyMap::from([("cwd".to_string(), ConfigKeyValue::from_str("sub"))]),
        }),
      ),
    ]);
    assert_eq!(
      get_trusted_config_map_hash(&config_map),
      "1:b446ed44effe92b6f8491f311559164f69b32f8c7fae77308f680d264749ccac"
    );
    assert_eq!(
      get_trusted_config_map_hash(&ConfigMap::new()),
      "1:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
  }
}
