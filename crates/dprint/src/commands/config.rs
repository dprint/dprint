use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use deno_terminal::colors;
use dprint_core::async_runtime::future;
use dprint_core::plugins;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::arg_parser::FilePatternArgs;
use crate::arg_parser::OutputResolvedConfigSubCommand;
use crate::configuration::get_init_config_file_text;
use crate::configuration::*;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::FetchNpmLatestInfo;
use crate::plugins::InfoFilePluginInfo;
use crate::plugins::PluginResolver;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginWrapper;
use crate::plugins::detect_npm_plugin_kind_in_node_modules;
use crate::plugins::fetch_npm_latest_info;
use crate::plugins::fetch_npm_latest_tarball_download;
use crate::plugins::fetch_npm_latest_tarball_info;
use crate::plugins::fetch_npm_versioned_tarball_download;
use crate::plugins::fetch_npm_versioned_tarball_info;
use crate::plugins::read_info_file;
use crate::plugins::read_update_url;
use crate::resolution::GetPluginResult;
use crate::resolution::ResolvePluginsScopeAndPathsOptions;
use crate::resolution::resolve_plugins_scope;
use crate::resolution::resolve_plugins_scope_and_paths;
use crate::utils::CachedDownloader;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::pretty_print_json_text;

pub struct InitConfigFileOptions<'a> {
  pub global: bool,
  pub config_arg: Option<&'a str>,
}

pub async fn init_config_file(environment: &impl Environment, options: InitConfigFileOptions<'_>) -> Result<()> {
  fn get_config_paths(environment: &impl Environment, options: &InitConfigFileOptions<'_>) -> Result<Vec<PathBuf>> {
    if options.global {
      let directory = crate::configuration::resolve_global_config_dir(environment).with_context(|| {
        concat!(
          "Could not find system config directory. ",
          "Maybe specify the DPRINT_CONFIG_DIR environment ",
          "variable to say where to store the global dprint configuration file."
        )
      })?;
      Ok(Vec::from([directory.join("dprint.jsonc"), directory.join("dprint.json")]))
    } else if let Some(config_arg) = options.config_arg {
      Ok(Vec::from([PathBuf::from(config_arg)]))
    } else {
      Ok(POSSIBLE_CONFIG_FILE_NAMES.iter().map(PathBuf::from).collect::<Vec<_>>())
    }
  }

  let mut config_file_paths = get_config_paths(environment, &options)?;
  for config_path in &config_file_paths {
    if environment.path_exists(config_path) {
      bail!("Configuration file '{}' already exists.", config_path.display())
    }
  }
  let config_file_path = config_file_paths.remove(0);
  let text = get_init_config_file_text(environment).await?;
  if let Some(parent) = config_file_path.parent() {
    _ = environment.mk_dir_all(parent);
  }
  environment.write_file(&config_file_path, &text)?;
  log_stdout_info!(environment, "\nCreated {}", config_file_path.display());
  if options.global {
    log_stdout_info!(environment, "\nRun `dprint config edit --global` to modify this file in the future.");
  }
  log_stdout_info!(
    environment,
    "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
  );
  Ok(())
}

pub async fn edit_config_file<TEnvironment: Environment>(args: &CliArgs, environment: &TEnvironment) -> Result<()> {
  let config_path_and_bytes = resolve_main_config_path_and_bytes(args, environment).await?.ok_or_else(|| {
    let is_global = args.config_discovery(environment).is_global();
    anyhow::anyhow!(
      "Could not find a configuration file. Create one with `dprint init{}`",
      if is_global { " --global" } else { "" }
    )
  })?;

  let config_path = match config_path_and_bytes.source {
    PathSource::Local(source) => source.path,
    PathSource::Remote(source) => {
      bail!("Cannot edit a remote configuration file '{}'", source.url)
    }
    PathSource::Npm(source) => {
      bail!("Cannot edit an npm configuration '{}'", source.specifier.display())
    }
  };

  let args = select_editor_args(environment)
    .into_iter()
    .map(OsString::from)
    .chain(std::iter::once(config_path.into_path_buf().into_os_string()))
    .collect::<Vec<OsString>>();
  let command_text_for_err = args
    .iter()
    .map(|s| format!("\"{}\"", s.to_string_lossy().replace("\"", "\\\"")))
    .collect::<Vec<_>>()
    .join(" ");
  let exit_code = environment
    .run_command_get_status(args)
    .with_context(|| format!("Failed to launch editor with command: {}", command_text_for_err))?;

  if let Some(exit_code) = exit_code.filter(|c| *c != 0) {
    // todo: use an exit code error
    bail!("Editor exited with code: {}", exit_code);
  }

  Ok(())
}

pub struct AddPluginsOptions<'a> {
  pub plugin_names_or_urls: &'a [String],
  /// Skip auto-pinning `dist-tags.latest` for `npm:` specifiers — write the
  /// unversioned form (deferring to node_modules / package.json).
  pub no_version: bool,
  /// In addition to writing the unversioned spec to dprint.json, add each
  /// `npm:` package to the nearest `package.json`'s `devDependencies`
  /// (as a caret range). Implies `no_version`.
  pub update_package_json: bool,
  /// Force a checksum onto each written entry, even for Wasm plugins (which
  /// are otherwise added without one). Mutually exclusive with `no_version` /
  /// `update_package_json`.
  pub checksum: bool,
}

pub async fn add_plugin_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  options: AddPluginsOptions<'_>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let AddPluginsOptions {
    plugin_names_or_urls,
    no_version,
    update_package_json,
    checksum,
  } = options;
  let config = resolve_config_from_args(args, environment).await?;
  let config_path = match config.source {
    PathSource::Local(source) => source.path,
    PathSource::Remote(_) | PathSource::Npm(_) => bail!("Cannot update plugins in a remote configuration."),
  };

  // Track npm packages we still need to write to package.json after the
  // config update succeeds. Walked-up package.json lookup happens once at
  // the end so a batch add only touches the file once.
  let mut package_json_additions: Vec<(String, String)> = Vec::new();
  // npm packages whose pre-existing config entry should be dropped before the
  // freshly-resolved specifier is appended (so re-adding replaces rather than
  // duplicates). Applied alongside the additions in the single read/write below.
  let mut npm_packages_to_replace: Vec<String> = Vec::new();

  let plugin_urls_to_add = if plugin_names_or_urls.is_empty() {
    if no_version || update_package_json {
      bail!("--no-version / --package-json require an explicit `npm:` specifier.");
    }
    let mut possible_plugins = get_possible_plugins_to_add(environment, plugin_resolver, config.plugins).await?;
    if possible_plugins.is_empty() {
      bail!("Could not find any plugins to add. Please provide one by specifying `dprint add <plugin-url>`.");
    }
    let index = environment.get_selection(
      "Select a plugin to add:",
      0,
      &possible_plugins.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
    )?;
    let selected = possible_plugins.remove(index);
    let (url, prefetched) = if checksum {
      ensure_url_checksum(selected.full_url(), environment).await?
    } else {
      (selected.full_url_no_wasm_checksum(), None)
    };
    vec![(url, prefetched)]
  } else {
    let mut urls = Vec::with_capacity(plugin_names_or_urls.len());
    for plugin_name_or_url in plugin_names_or_urls {
      if let Some(resolved) = resolve_plugin_url_to_add(
        ResolvePluginUrlOptions {
          plugin_name_or_url,
          config_path: &config_path,
          config_plugins: &config.plugins,
          no_version,
          update_package_json,
          checksum,
        },
        &mut package_json_additions,
        &mut npm_packages_to_replace,
        environment,
        plugin_resolver,
      )
      .await?
      {
        urls.push(resolved);
      }
    }
    urls
  };

  let urls_only = plugin_urls_to_add.iter().map(|(url, _)| url.clone()).collect::<Vec<_>>();
  let file_text = environment.read_file(&config_path)?;
  let file_text = add_plugins_to_config(&file_text, &npm_packages_to_replace, &urls_only)?;
  environment.write_file(&config_path, &file_text)?;

  if update_package_json && !package_json_additions.is_empty() {
    apply_package_json_additions(&config_path, &package_json_additions, environment)?;
  }

  // reuse anything we already downloaded while resolving (to compute a
  // checksum) to populate the plugin cache, so the first `dprint fmt` is a
  // cache hit instead of downloading and compiling it again. Best-effort: a
  // failure here just means the plugin gets set up lazily on first use.
  setup_prefetched_plugins(&config_path, plugin_urls_to_add, environment, plugin_resolver).await;

  Ok(())
}

/// Populates the plugin cache from the bytes captured during resolution (see
/// the `prefetched` payloads). Failures are logged at debug and otherwise
/// ignored — pre-caching is an optimization, not part of the add's contract.
async fn setup_prefetched_plugins<TEnvironment: Environment>(
  config_path: &CanonicalizedPathBuf,
  resolved: Vec<(String, Option<Vec<u8>>)>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) {
  let base = PathSource::new_local(config_path.clone());
  for (url, prefetched) in resolved {
    let Some(bytes) = prefetched else { continue };
    let reference = match crate::plugins::parse_plugin_source_reference(&url, &base, environment) {
      Ok(reference) => reference,
      Err(err) => {
        log_debug!(environment, "Skipping pre-cache of {}: {:#}", url, err);
        continue;
      }
    };
    if let Err(err) = plugin_resolver.setup_from_prefetched_download(&reference, bytes).await {
      log_debug!(environment, "Failed to pre-cache {}: {:#}", url, err);
    }
  }
}

/// Inputs to [`resolve_plugin_url_to_add`]. Bundled to keep the function
/// signature manageable; the environment and plugin resolver are passed
/// alongside as services rather than fields here.
struct ResolvePluginUrlOptions<'a> {
  plugin_name_or_url: &'a str,
  config_path: &'a CanonicalizedPathBuf,
  config_plugins: &'a [PluginSourceReference],
  no_version: bool,
  update_package_json: bool,
  checksum: bool,
}

/// What `resolve_npm_plugin_to_add` decided to write into the config plus
/// (when `--package-json` was set and the package needed pinning) the
/// devDependencies entry the caller should queue.
#[derive(Debug)]
struct ResolvedNpmPluginAdd {
  url: String,
  /// The package name, so the caller can drop any pre-existing entry for the
  /// same package before appending this one.
  package_name: String,
  package_json_addition: Option<(String, String)>,
  /// The package tarball downloaded to compute a checksum, if any — handed to
  /// the plugin cache so the first `dprint fmt` doesn't re-download it.
  prefetched_tarball: Option<Vec<u8>>,
}

/// Resolves a plugin name or URL to a plugin URL to add to the config.
///
/// Returns `Some((url, prefetched_bytes))` for new plugins, or `None` if the
/// plugin was already present and was updated in-place. `prefetched_bytes` is
/// `Some` when we downloaded the plugin (to compute a checksum) and can reuse
/// the bytes to warm the plugin cache. When the input is an `npm:` specifier
/// resolved with `--package-json`, the caller queues the returned
/// devDependencies entry via `package_json_additions`.
async fn resolve_plugin_url_to_add<TEnvironment: Environment>(
  options: ResolvePluginUrlOptions<'_>,
  package_json_additions: &mut Vec<(String, String)>,
  npm_packages_to_replace: &mut Vec<String>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<Option<(String, Option<Vec<u8>>)>> {
  let ResolvePluginUrlOptions {
    plugin_name_or_url,
    config_path,
    config_plugins,
    no_version,
    update_package_json,
    checksum,
  } = options;
  // intercept npm: specifiers before URL parsing. For unversioned forms,
  // defer to package.json devDependencies if present (so npm/node_modules
  // manages the version), otherwise resolve dist-tags.latest and compute
  // the tarball checksum for process plugins. Url::parse would otherwise
  // accept `npm:foo` as a valid URL and pass it through without pinning.
  if plugin_name_or_url.starts_with("npm:") {
    let resolved = resolve_npm_plugin_to_add(
      ResolveNpmPluginOptions {
        text: plugin_name_or_url,
        config_path,
        no_version,
        update_package_json,
        checksum,
      },
      environment,
    )
    .await?;
    if let Some(addition) = resolved.package_json_addition {
      package_json_additions.push(addition);
    }
    // drop any pre-existing entry for the same package (any version) so the
    // freshly-resolved specifier replaces it rather than appending a
    // duplicate. The caller prunes these names from the config in the same
    // read/write that appends `resolved.url`.
    npm_packages_to_replace.push(resolved.package_name);
    return Ok(Some((resolved.url, resolved.prefetched_tarball)));
  }
  if no_version || update_package_json {
    bail!("--no-version / --package-json only apply to `npm:` specifiers (got '{}').", plugin_name_or_url);
  }
  match Url::parse(plugin_name_or_url) {
    Ok(url) => {
      let url = url.to_string();
      let (url, prefetched) = if checksum {
        ensure_url_checksum(url, environment).await?
      } else {
        (url, None)
      };
      Ok(Some((url, prefetched)))
    }
    Err(_) => {
      let cached_downloader = CachedDownloader::new(environment.clone());
      let plugin_name = if plugin_name_or_url.contains('/') {
        plugin_name_or_url.to_string()
      } else {
        format!("dprint/{}", plugin_name_or_url)
      };
      let plugin = match read_update_url(
        &cached_downloader,
        &Url::parse(&format!("https://plugins.dprint.dev/{}/latest.json", plugin_name))?,
      )
      .await?
      {
        Some(result) => result,
        None => {
          let trailing_message = if let Ok(possible_plugins) = get_possible_plugins_to_add(environment, plugin_resolver, config_plugins.to_vec()).await {
            if possible_plugins.is_empty() {
              String::new()
            } else {
              format!(
                "\n\nPlugins:\n{}",
                possible_plugins.iter().map(|p| format!(" * {}", p.name)).collect::<Vec<_>>().join("\n")
              )
            }
          } else {
            String::new()
          };
          bail!(
            "Could not find plugin with name '{}'. Please fix the name or try a url instead.{}",
            plugin_name_or_url,
            trailing_message,
          )
        }
      };
      for (config_plugin_reference, config_plugin) in get_config_file_plugins(plugin_resolver, config_plugins.to_vec()).await {
        if let Ok(config_plugin) = config_plugin
          && let Some(update_url) = &config_plugin.info().update_url
          && let Ok(update_url) = Url::parse(update_url)
          && let Ok(Some(config_plugin_latest)) = read_update_url(&cached_downloader, &update_url).await
        {
          // if two plugins have the same URL to be updated to then they're the same plugin
          if config_plugin_latest.url == plugin.url {
            let file_text = environment.read_file(config_path)?;
            let new_reference = plugin.as_source_reference()?;
            let file_text = update_plugin_in_config(
              &file_text,
              &PluginUpdateInfo {
                name: config_plugin.info().name.to_string(),
                old_version: config_plugin.info().version.to_string(),
                old_reference: config_plugin_reference,
                new_version: plugin.version,
                new_reference,
              },
            );
            environment.write_file(config_path, &file_text)?;
            return Ok(None);
          }
        }
      }
      if checksum {
        let (url, prefetched) = ensure_url_checksum(plugin.full_url(), environment).await?;
        Ok(Some((url, prefetched)))
      } else {
        Ok(Some((plugin.full_url_no_wasm_checksum(), None)))
      }
    }
  }
}

/// Ensures a resolved plugin URL carries a checksum, downloading the file to
/// compute one when the registry/info didn't already supply it. Returns the
/// (possibly checksummed) URL plus the downloaded bytes when a download
/// happened, so the caller can reuse them to warm the plugin cache. Only
/// wasm/json plugin URLs can be checksummed; any other URL is returned
/// unchanged with no bytes.
async fn ensure_url_checksum(url: String, environment: &impl Environment) -> Result<(String, Option<Vec<u8>>)> {
  let parsed = crate::utils::parse_checksum_path_or_url(&url);
  if parsed.checksum.is_some() {
    return Ok((url, None));
  }
  let lower = parsed.path_or_url.to_lowercase();
  if !(lower.ends_with(".wasm") || lower.ends_with(".json")) {
    return Ok((url, None)); // not a plugin file we can checksum
  }
  let parsed_url = Url::parse(&parsed.path_or_url)?;
  let (_, file) = environment.download_file_err_404(&parsed_url, None).await?;
  let bytes = file.content;
  let url = format!("{}@{}", parsed.path_or_url, crate::utils::get_sha256_checksum(&bytes));
  Ok((url, Some(bytes)))
}

struct ResolveNpmPluginOptions<'a> {
  text: &'a str,
  config_path: &'a CanonicalizedPathBuf,
  no_version: bool,
  update_package_json: bool,
  /// Force a checksum onto the written entry even for Wasm plugins. Process
  /// plugins always carry one regardless.
  checksum: bool,
}

/// Resolves an `npm:` specifier from `dprint add` into the string to write
/// into the config's `plugins` array, plus the devDependencies entry to
/// queue when `--package-json` was set.
///
/// When the user didn't write an explicit plugin path, dprint inspects the
/// package to detect whether it's a wasm (`plugin.wasm`) or process
/// (`plugin.json`) plugin and writes the matching form — for the pinned forms
/// it downloads the tarball; for the unversioned forms it reads `node_modules`.
///
/// With `--checksum` (mutually exclusive with `--no-version` / `--package-json`)
/// a checksum is forced onto the written entry even for Wasm plugins, and an
/// otherwise-deferred unversioned add is pinned instead (since a checksum
/// requires a version).
///
/// - `npm:foo@1.2.3/plugin.json[@sha]` (explicit path) → pass through verbatim
///   (unless `--checksum` and it has none, in which case one is computed).
/// - `npm:foo@1.2.3` (versioned, no path) → download that version's tarball to
///   detect the kind and pin the matching form (with checksum for process).
/// - `npm:foo` with `--no-version` or `--package-json` → keep unversioned. With
///   `--package-json`, also return a `devDependencies` entry for the nearest
///   `package.json` (caret range pinning the resolved latest) and detect the
///   kind from that tarball; with bare `--no-version` the registry isn't
///   touched, so the kind is read from `node_modules` if installed.
/// - `npm:foo` (unversioned) and the package is in a nearby `package.json`'s
///   `devDependencies` → keep unversioned (defer to npm/node_modules), reading
///   the kind from `node_modules` if installed.
/// - `npm:foo` (unversioned) otherwise → resolve `dist-tags.latest`, download
///   the tarball to detect the kind, and write the pinned form (with checksum
///   for process plugins).
async fn resolve_npm_plugin_to_add(options: ResolveNpmPluginOptions<'_>, environment: &impl Environment) -> Result<ResolvedNpmPluginAdd> {
  let ResolveNpmPluginOptions {
    text,
    config_path,
    no_version,
    update_package_json,
    checksum,
  } = options;
  let parsed = crate::utils::parse_npm_specifier(text)?;
  // when the user wrote an explicit plugin path, enforce the same .wasm/.json
  // constraint parse_plugin_source_reference applies, since `dprint add` writes
  // the result straight into the plugins array. When the path was defaulted we
  // detect the real kind below, so there's nothing to validate yet.
  let explicit_path = parsed.path_was_explicit;
  if explicit_path {
    crate::utils::validate_plugin_extension(&parsed.specifier, text)?;
  }

  // a config path with no parent shouldn't happen — `resolve_config_from_args`
  // hands us a canonicalized file path. Treat it as a hard bug rather than a
  // soft fall-through that silently skips the package.json walk.
  let start_dir = config_path
    .parent()
    .ok_or_else(|| anyhow!("Config path {} has no parent directory.", config_path.display()))?;
  let start_dir_ref: &Path = start_dir.as_ref();
  let name = parsed.specifier.name.clone();

  if let Some(version) = &parsed.specifier.version {
    if no_version {
      bail!("--no-version cannot be combined with a versioned specifier: {}", text);
    }
    // pass the user's spec through verbatim when there's nothing to add: it
    // already carries a checksum, or it names a plugin file and we aren't
    // forcing one.
    if parsed.checksum.is_some() || (explicit_path && !checksum) {
      return Ok(ResolvedNpmPluginAdd {
        url: text.to_string(),
        package_name: name,
        package_json_addition: None,
        prefetched_tarball: None,
      });
    }
    // otherwise download the tarball to compute the checksum (and, for a
    // defaulted path, detect the plugin kind), then write the matching entry.
    let (path, tarball_sha256, tarball_bytes) = if explicit_path {
      // the user named the plugin file, so don't inspect the package — that
      // would wrongly require a root plugin.wasm/plugin.json.
      let download = fetch_npm_versioned_tarball_download(&name, version, Some(start_dir_ref), environment).await?;
      (parsed.specifier.path.clone(), download.tarball_sha256, download.tarball_bytes)
    } else {
      let info = fetch_npm_versioned_tarball_info(&name, version, Some(start_dir_ref), environment).await?;
      (default_plugin_path(info.plugin_kind).to_string(), info.tarball_sha256, info.tarball_bytes)
    };
    let specifier = crate::utils::NpmSpecifier {
      name,
      version: Some(version.clone()),
      path,
    };
    let url = npm_add_url(&specifier, &tarball_sha256, checksum);
    return Ok(ResolvedNpmPluginAdd {
      url,
      package_name: specifier.name,
      package_json_addition: None,
      prefetched_tarball: tarball_bytes,
    });
  }

  if no_version {
    // Look up the latest version so we can write a caret range. If the
    // registry is unreachable we still write the unversioned spec to
    // dprint.json but bail on the package.json update so the user notices
    // (rather than ending up with an out-of-sync state). When the path was
    // defaulted we also detect the kind from that tarball — `--package-json`
    // is usually run before the package is installed, so node_modules
    // detection alone wouldn't find it.
    let mut detected_kind = None;
    let package_json_addition = if update_package_json {
      let version = if explicit_path {
        // an explicit path already tells us the kind; only the version is needed.
        fetch_npm_latest_info(
          FetchNpmLatestInfo {
            specifier: &parsed.specifier,
            start_dir: Some(start_dir_ref),
            want_tarball_sha: false,
          },
          environment,
        )
        .await
        .with_context(|| format!("Resolving latest version for package.json entry of {}", name))?
        .version
      } else {
        let info = fetch_npm_latest_tarball_info(&name, Some(start_dir_ref), environment)
          .await
          .with_context(|| format!("Resolving latest version for package.json entry of {}", name))?;
        detected_kind = Some(info.plugin_kind);
        info.version
      };
      Some((name.clone(), format!("^{}", version)))
    } else {
      None
    };
    return Ok(ResolvedNpmPluginAdd {
      url: unversioned_npm_add_url(&parsed, explicit_path, detected_kind, start_dir_ref, environment),
      package_name: name,
      package_json_addition,
      // unversioned specifiers resolve from node_modules, so there's nothing to
      // pre-cache (any tarball we fetched above was only for the caret range).
      prefetched_tarball: None,
    });
  }

  // `--checksum` forces a pinned, checksummed entry, so don't defer to the
  // unversioned node_modules form when it's set.
  if !checksum && is_in_package_json_deps(&name, start_dir_ref, environment) {
    log_stderr_info!(environment, "Found {} in package.json — adding unversioned npm specifier.", name);
    return Ok(ResolvedNpmPluginAdd {
      url: unversioned_npm_add_url(&parsed, explicit_path, None, start_dir_ref, environment),
      package_name: name,
      package_json_addition: None,
      prefetched_tarball: None,
    });
  }

  if explicit_path {
    // the user named the plugin file; pin the latest version and add a checksum
    // for non-wasm plugins (and for wasm too when `--checksum` was passed). Only
    // download the tarball when a checksum is actually needed — a wasm explicit
    // path without `--checksum` just needs the version.
    let needs_tarball = checksum || parsed.specifier.plugin_kind() == PluginKind::Process;
    let (version, sha_and_bytes) = if needs_tarball {
      // explicit path → don't inspect the package for a kind, just download.
      let download = fetch_npm_latest_tarball_download(&name, Some(start_dir_ref), environment).await?;
      (download.version, Some((download.tarball_sha256, download.tarball_bytes)))
    } else {
      let info = fetch_npm_latest_info(
        FetchNpmLatestInfo {
          specifier: &parsed.specifier,
          start_dir: Some(start_dir_ref),
          want_tarball_sha: false,
        },
        environment,
      )
      .await?;
      (info.version, None)
    };
    let pinned = crate::utils::NpmSpecifier {
      name,
      version: Some(version),
      path: parsed.specifier.path,
    };
    let display = pinned.display();
    let (url, prefetched_tarball) = match sha_and_bytes {
      Some((sha, bytes)) => (format!("{}@{}", display, sha), bytes),
      None => (display, None),
    };
    return Ok(ResolvedNpmPluginAdd {
      url,
      package_name: pinned.name,
      package_json_addition: None,
      prefetched_tarball,
    });
  }

  // no path given: resolve the latest version and inspect its tarball to learn
  // the plugin kind (and checksum, required for process plugins / `--checksum`).
  let info = fetch_npm_latest_tarball_info(&name, Some(start_dir_ref), environment).await?;
  let specifier = crate::utils::NpmSpecifier {
    name,
    version: Some(info.version),
    path: default_plugin_path(info.plugin_kind).to_string(),
  };
  let url = npm_add_url(&specifier, &info.tarball_sha256, checksum);
  Ok(ResolvedNpmPluginAdd {
    url,
    package_name: specifier.name,
    package_json_addition: None,
    prefetched_tarball: info.tarball_bytes,
  })
}

/// The default file name within an npm package for each plugin kind.
fn default_plugin_path(kind: PluginKind) -> &'static str {
  match kind {
    PluginKind::Wasm => "plugin.wasm",
    PluginKind::Process => "plugin.json",
  }
}

/// Joins a resolved npm specifier with its tarball checksum when one should be
/// written: process plugins always require it; wasm plugins only when the user
/// passed `--checksum` (`force_checksum`).
fn npm_add_url(specifier: &crate::utils::NpmSpecifier, tarball_sha256: &str, force_checksum: bool) -> String {
  let display = specifier.display();
  if force_checksum || specifier.plugin_kind() == PluginKind::Process {
    format!("{}@{}", display, tarball_sha256)
  } else {
    display
  }
}

/// Picks the unversioned specifier string to write for an npm plugin we aren't
/// pinning (deferring to node_modules / package.json). When the user didn't
/// give a path, use `detected_kind` if the caller already learned it from the
/// registry tarball, else fall back to inspecting `node_modules`, so a process
/// plugin gets `/plugin.json`; otherwise keep the bare form.
fn unversioned_npm_add_url(
  parsed: &crate::utils::ParsedNpmSpecifier,
  explicit_path: bool,
  detected_kind: Option<PluginKind>,
  start_dir: &Path,
  environment: &impl Environment,
) -> String {
  if explicit_path {
    return parsed.specifier.display();
  }
  let kind = detected_kind.or_else(|| detect_npm_plugin_kind_in_node_modules(&parsed.specifier.name, start_dir, environment));
  let path = match kind {
    Some(PluginKind::Process) => "plugin.json".to_string(),
    _ => parsed.specifier.path.clone(),
  };
  crate::utils::NpmSpecifier {
    name: parsed.specifier.name.clone(),
    version: None,
    path,
  }
  .display()
}

/// Writes new `devDependencies` entries into the nearest `package.json`
/// (walking up from the dprint config). Updates an existing entry in place;
/// appends new ones. Warns and skips the update if no `package.json` is
/// anywhere along the walk — the dprint.json change is still saved so the
/// user only has to add a `package.json` (or rerun with no `--package-json`)
/// to recover; bailing would leave them with a partially-applied add.
fn apply_package_json_additions(config_path: &CanonicalizedPathBuf, additions: &[(String, String)], environment: &impl Environment) -> Result<()> {
  use jsonc_parser::cst::CstRootNode;
  use jsonc_parser::json;

  let start_dir = config_path
    .parent()
    .ok_or_else(|| anyhow!("Config path {} has no parent directory.", config_path.display()))?;
  let mut pkg_path = None;
  for dir in start_dir.as_ref().ancestors() {
    let candidate = dir.join("package.json");
    if environment.path_exists(&candidate) {
      pkg_path = Some(candidate);
      break;
    }
  }
  let Some(pkg_path) = pkg_path else {
    log_warn!(
      environment,
      "Skipped package.json update: no package.json was found at or above {}. Run `npm init -y` and re-run `dprint add --package-json` to record {} entr{}.",
      start_dir.display(),
      additions.len(),
      if additions.len() == 1 { "y" } else { "ies" },
    );
    return Ok(());
  };

  let text = environment.read_file(&pkg_path)?;
  let root = CstRootNode::parse(&text, &Default::default()).with_context(|| format!("Failed parsing {}", pkg_path.display()))?;
  let root_obj = root.object_value_or_set();
  let dev_deps = root_obj.object_value_or_set("devDependencies");
  dev_deps.ensure_multiline();
  for (name, range) in additions {
    match dev_deps.get(name) {
      Some(existing) => existing.set_value(json!(range.clone())),
      None => {
        dev_deps.append(name, json!(range.clone()));
      }
    }
  }
  environment.write_file(&pkg_path, &root.to_string())?;
  log_stderr_info!(
    environment,
    "Updated {} with {} new devDependencies entr{}. Run `npm install` to install them.",
    pkg_path.display(),
    additions.len(),
    if additions.len() == 1 { "y" } else { "ies" },
  );
  Ok(())
}

/// Returns true if any `package.json` found walking up from `start_dir` lists
/// `package_name` under `dependencies` or `devDependencies`. Monorepos
/// commonly list deps at the workspace root rather than each package, so we
/// keep climbing past package.jsons that don't mention the plugin.
/// Malformed `package.json`s along the way are warned about (it's almost
/// certainly a mistake the user wants to know about) and then skipped.
fn is_in_package_json_deps(package_name: &str, start_dir: &std::path::Path, environment: &impl Environment) -> bool {
  use jsonc_parser::JsonValue;
  use jsonc_parser::parse_to_value;

  for dir in start_dir.ancestors() {
    let pkg_path = dir.join("package.json");
    let Ok(text) = environment.read_file(&pkg_path) else {
      continue;
    };
    let parsed = match parse_to_value(&text, &Default::default()) {
      Ok(Some(JsonValue::Object(obj))) => obj,
      Ok(_) => {
        // not an object (e.g. an array or scalar); skip but warn
        log_warn!(environment, "Skipping {}: top-level value is not an object.", pkg_path.display());
        continue;
      }
      Err(err) => {
        log_warn!(environment, "Skipping {}: failed to parse ({:#}).", pkg_path.display(), err);
        continue;
      }
    };
    for field in ["dependencies", "devDependencies"] {
      if let Some(JsonValue::Object(deps)) = parsed.get(field)
        && deps.get(package_name).is_some()
      {
        return true;
      }
    }
  }
  false
}

async fn get_possible_plugins_to_add<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  current_plugins: Vec<PluginSourceReference>,
) -> Result<Vec<InfoFilePluginInfo>> {
  let info_file = read_info_file(environment)
    .await
    .map_err(|err| anyhow!("Failed downloading info file. {:#}", err))?;
  let current_plugin_names = get_config_file_plugins(plugin_resolver, current_plugins)
    .await
    .into_iter()
    .filter_map(|(plugin_reference, plugin_result)| match plugin_result {
      Ok(plugin) => Some(plugin.info().name.to_string()),
      Err(err) => {
        log_warn!(environment, "Failed resolving plugin: {}\n\n{:#}", plugin_reference.path_source.display(), err);
        None
      }
    })
    .collect::<HashSet<_>>();
  Ok(
    info_file
      .latest_plugins
      .into_iter()
      .filter(|p| !current_plugin_names.contains(&p.name))
      .collect(),
  )
}

pub struct UpdatePluginsOptions {
  /// Upgrade process plugins without prompting to confirm their new checksums.
  pub yes_to_prompts: bool,
  /// Print the updates that would be made without modifying any files.
  pub dry_run: bool,
}

pub async fn update_plugins_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  options: UpdatePluginsOptions,
) -> Result<()> {
  let UpdatePluginsOptions { yes_to_prompts, dry_run } = options;
  if !args.plugins.is_empty() {
    bail!("Cannot specify plugins for this sub command. Sorry, too much work for me.");
  }

  let file_pattern_args = FilePatternArgs {
    include_patterns: Vec::new(),
    include_pattern_overrides: None,
    exclude_patterns: Vec::new(),
    exclude_pattern_overrides: None,
    allow_node_modules: false,
    no_gitignore: false,
    only_staged: false,
    only_dirty: false,
  };
  let config_discovery = args.config_discovery(environment);
  let scopes = resolve_plugins_scope_and_paths(
    args,
    &file_pattern_args,
    environment,
    plugin_resolver,
    ResolvePluginsScopeAndPathsOptions {
      skip_traversal: config_discovery.is_global(),
    },
  )
  .await?;
  let mut plugin_responses = HashMap::new();
  let mut updates_per_scope = HashMap::with_capacity(scopes.len());
  // for `--dry-run`: the would-be config text per scope (plugin urls bumped),
  // kept in memory so the config-update preview can run against it without
  // touching disk.
  let mut dry_run_texts = HashMap::new();
  for (i, scope) in scopes.iter().enumerate() {
    let is_main_config = i == 0;
    let Some(config) = &scope.scope.config else {
      continue;
    };
    let config_path = match &config.source {
      PathSource::Local(source) => &source.path,
      PathSource::Remote(_) | PathSource::Npm(_) => {
        log_warn!(environment, "Skipping non-local configuration file: {}", config.source.display());
        continue;
      }
    };

    let mut file_text = environment.read_file(config_path)?;
    let plugins_to_update = get_plugins_to_update(environment, plugin_resolver, config.plugins.clone()).await?;

    let mut updated_plugins = Vec::with_capacity(plugins_to_update.len());
    for result in plugins_to_update {
      match result {
        Ok(info) => {
          // in a dry run nothing is written, so don't prompt to confirm
          // process plugin checksums — just report everything that would update
          let should_update = if info.is_wasm() || yes_to_prompts || dry_run {
            true
          } else if let Some(previous_response) = plugin_responses.get(&info.new_reference) {
            *previous_response
          } else {
            // prompt for security reasons
            log_all!(
              environment,
              "The process plugin {} {} has a new url: {}",
              info.name,
              info.old_version,
              info.get_full_new_config_url(),
            );
            let response = environment.confirm("Do you want to update it?", false)?;
            plugin_responses.insert(info.new_reference.clone(), response);
            response
          };

          if should_update {
            let in_config = if is_main_config {
              String::new()
            } else {
              format!(" in {}", config_path.display())
            };
            if dry_run {
              log_stderr_info!(
                environment,
                "Would update {} {}{} to {}.",
                colors::bold(&info.name),
                info.old_version,
                in_config,
                info.new_version
              );
            } else {
              log_stderr_info!(
                environment,
                "Updating {} {}{} to {}...",
                info.name,
                info.old_version,
                in_config,
                info.new_version
              );
            }
            file_text = update_plugin_in_config(&file_text, &info);
            updated_plugins.push(info);
          }
        }
        Err(err_info) => {
          log_warn!(environment, "Failed updating plugin {}: {:#}", err_info.name, err_info.error);
        }
      }
    }

    updates_per_scope.insert(config_path.clone(), updated_plugins);

    if dry_run {
      dry_run_texts.insert(config_path.clone(), file_text);
    } else {
      environment.write_file(config_path, &file_text)?;
    }
  }

  if dry_run {
    return preview_plugin_config_updates(environment, plugin_resolver, &updates_per_scope, &dry_run_texts).await;
  }

  // now resolve the plugins again in every scope and run their config updates

  run_plugin_config_updates(environment, args, &file_pattern_args, plugin_resolver, &updates_per_scope)
    .await
    .with_context(|| "Failed running plugin config updates.".to_string())?;

  Ok(())
}

/// Dry-run counterpart of [`run_plugin_config_updates`]: resolves each updated
/// plugin's new reference, asks it what config changes it would make, applies
/// them to the in-memory (already plugin-url-bumped) config text, then prints
/// the resulting file instead of writing it. No files are modified.
async fn preview_plugin_config_updates<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  updates_per_scope: &HashMap<CanonicalizedPathBuf, Vec<PluginUpdateInfo>>,
  dry_run_texts: &HashMap<CanonicalizedPathBuf, String>,
) -> Result<()> {
  let mut any_updates = false;
  // sort for deterministic output — `updates_per_scope` is a HashMap
  let mut config_paths = updates_per_scope.keys().collect::<Vec<_>>();
  config_paths.sort_by_key(|p| p.display().to_string());
  for config_path in config_paths {
    let updated_plugins = &updates_per_scope[config_path];
    if updated_plugins.is_empty() {
      continue;
    }
    any_updates = true;
    let Some(mut file_text) = dry_run_texts.get(config_path).cloned() else {
      continue;
    };
    let config_map = match deserialize_config_raw(&file_text) {
      Ok(map) => map,
      Err(err) => {
        log_warn!(environment, "Failed deserializing config file '{}': {:#}", config_path.display(), err);
        continue;
      }
    };
    let mut all_diagnostics = Vec::new();
    for update_info in updated_plugins {
      let plugin = match plugin_resolver.resolve_plugin(update_info.new_reference.clone()).await {
        Ok(plugin) => plugin,
        Err(err) => {
          log_warn!(environment, "Failed resolving {}. {:#}", update_info.name, err);
          continue;
        }
      };
      let config_key = &plugin.info().config_key;
      let Some(plugin_config) = config_map.get(config_key).and_then(|c| c.as_object()).cloned() else {
        continue;
      };
      let initialized_plugin = match plugin.initialize().await {
        Ok(plugin) => plugin,
        Err(err) => {
          log_warn!(environment, "Failed initializing {}. {:#}", update_info.name, err);
          continue;
        }
      };

      let changes = match initialized_plugin
        .check_config_updates(plugins::CheckConfigUpdatesMessage {
          old_version: Some(update_info.old_version.clone()),
          config: plugin_config,
        })
        .await
      {
        Ok(changes) => changes,
        Err(err) => {
          log_warn!(environment, "Failed applying update config changes for {}. {:#}", update_info.name, err);
          continue;
        }
      };

      if changes.is_empty() {
        continue;
      }

      let result = apply_config_changes(&file_text, config_key, &changes);
      all_diagnostics.extend(result.diagnostics);
      file_text = result.new_text;
    }

    if !all_diagnostics.is_empty() {
      log_warn!(environment, "Had diagnostics applying update config changes for {}:", config_path.display());
      for diagnostic in &all_diagnostics {
        log_warn!(environment, "* {}", diagnostic);
      }
    }

    log_stdout_info!(
      environment,
      "\n{}\n{}",
      colors::bold(format!("{} would be updated to:", config_path.display())),
      file_text
    );
  }

  if any_updates {
    log_stderr_info!(environment, "\n{}", colors::gray("This was a dry run. No files were changed."));
  } else {
    log_stderr_info!(environment, "{}", colors::gray("No plugin updates available."));
  }

  Ok(())
}

async fn run_plugin_config_updates<TEnvironment: Environment>(
  environment: &TEnvironment,
  args: &CliArgs,
  file_pattern_args: &FilePatternArgs,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  updates_per_scope: &HashMap<CanonicalizedPathBuf, Vec<PluginUpdateInfo>>,
) -> Result<()> {
  let config_discovery = args.config_discovery(environment);
  let scopes = resolve_plugins_scope_and_paths(
    args,
    file_pattern_args,
    environment,
    plugin_resolver,
    ResolvePluginsScopeAndPathsOptions {
      skip_traversal: config_discovery.is_global(),
    },
  )
  .await?;
  for scope in scopes.into_iter() {
    let Some(config) = &scope.scope.config else {
      continue;
    };
    let config_path = match &config.source {
      PathSource::Local(source) => &source.path,
      PathSource::Remote(_) | PathSource::Npm(_) => {
        continue;
      }
    };
    let updated_plugins = match updates_per_scope.get(config_path) {
      Some(updates) => updates,
      None => {
        continue;
      }
    };
    if updated_plugins.is_empty() {
      continue;
    }
    let mut file_text = environment.read_file(config_path)?;
    let config_map = match deserialize_config_raw(&file_text) {
      Ok(map) => map,
      Err(err) => {
        log_warn!(environment, "Failed deserializing config file '{}': {:#}", config_path.display(), err);
        continue;
      }
    };
    let mut all_diagnostics = Vec::new();
    for plugin in scope.scope.plugins.values() {
      let Some(update_info) = updated_plugins
        .iter()
        .find(|info| info.name == plugin.info().name && info.new_version == plugin.info().version)
      else {
        continue;
      };
      log_debug!(environment, "Updating for {}", plugin.name());
      let config_key = &plugin.info().config_key;
      let Some(plugin_config) = config_map.get(config_key).and_then(|c| c.as_object()).cloned() else {
        continue;
      };
      let initialized_plugin = match plugin.initialize().await {
        Ok(plugin) => plugin,
        Err(err) => {
          log_warn!(environment, "Failed initializing {}. {:#}", update_info.name, err);
          continue;
        }
      };

      let changes = match initialized_plugin
        .check_config_updates(plugins::CheckConfigUpdatesMessage {
          old_version: Some(update_info.old_version.clone()),
          config: plugin_config,
        })
        .await
      {
        Ok(changes) => changes,
        Err(err) => {
          log_warn!(environment, "Failed applying update config changes for {}. {:#}", update_info.name, err);
          continue;
        }
      };

      log_debug!(environment, "Had {} changes.", changes.len());
      if changes.is_empty() {
        continue;
      }

      let result = apply_config_changes(&file_text, config_key, &changes);
      all_diagnostics.extend(result.diagnostics);
      file_text = result.new_text;
    }

    // apply the changes to the config
    if !all_diagnostics.is_empty() {
      log_warn!(environment, "Had diagnostics applying update config changes for {}:", config_path.display());
      for diagnostic in &all_diagnostics {
        log_warn!(environment, "* {}", diagnostic);
      }
    }
    environment.write_file(config_path, &file_text)?;
  }
  Ok(())
}

struct PluginUpdateError {
  name: String,
  error: Error,
}

async fn get_plugins_to_update<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  plugins: Vec<PluginSourceReference>,
) -> Result<Vec<Result<PluginUpdateInfo, PluginUpdateError>>> {
  async fn resolve_plugin_update_info<TEnvironment: Environment>(
    environment: &TEnvironment,
    plugin_reference: PluginSourceReference,
    plugin_result: Result<Rc<PluginWrapper>>,
  ) -> Option<Result<PluginUpdateInfo, PluginUpdateError>> {
    let plugin = match plugin_result {
      Ok(plugin) => plugin,
      Err(error) => {
        return Some(Err(PluginUpdateError {
          name: plugin_reference.path_source.display(),
          error,
        }));
      }
    };

    // npm specifiers update via the npm registry (dist-tags.latest), not the
    // plugin's update_url which would migrate us off the npm form
    if let PathSource::Npm(npm_source) = &plugin_reference.path_source {
      if npm_source.specifier.version.is_none() {
        // unversioned specifiers track node_modules — versions are managed by npm/package-lock
        log_warn!(
          environment,
          "Skipping {} (unversioned npm specifier — update via your package manager).",
          plugin.info().name
        );
        return None;
      }
      let start_dir = npm_source.base_dir.as_ref().map(|d| d.as_ref());
      // preserve the user's checksum on update: if they pinned a checksum on
      // the old reference, fetch a fresh one for the new version instead of
      // carrying the stale hash (which would fail verification on next run)
      let args = FetchNpmLatestInfo {
        specifier: &npm_source.specifier,
        start_dir,
        want_tarball_sha: plugin_reference.checksum.is_some(),
      };
      match fetch_npm_latest_info(args, environment).await {
        Ok(info) => {
          let new_specifier = crate::utils::NpmSpecifier {
            name: npm_source.specifier.name.clone(),
            version: Some(info.version.clone()),
            path: npm_source.specifier.path.clone(),
          };
          let new_reference = PluginSourceReference {
            path_source: PathSource::new_npm(new_specifier, npm_source.base_dir.clone()),
            checksum: info.tarball_sha256,
          };
          return Some(Ok(PluginUpdateInfo {
            name: plugin.info().name.to_string(),
            old_version: plugin.info().version.to_string(),
            old_reference: plugin_reference,
            new_version: info.version,
            new_reference,
          }));
        }
        Err(err) => {
          return Some(Err(PluginUpdateError {
            name: plugin_reference.path_source.display(),
            error: err,
          }));
        }
      }
    }

    // request
    if let Some(plugin_update_url) = &plugin.info().update_url {
      match Url::parse(plugin_update_url) {
        Ok(update_url) => {
          match read_update_url(environment, &update_url).await.and_then(|result| match result {
            Some(info) => match info.as_source_reference() {
              Ok(source_reference) => Ok((info, source_reference)),
              Err(err) => Err(err),
            },
            None => Err(anyhow!("Failed downloading {} - 404 Not Found", update_url)),
          }) {
            Ok((info, new_reference)) => Some(Ok(PluginUpdateInfo {
              name: plugin.info().name.to_string(),
              old_reference: plugin_reference,
              old_version: plugin.info().version.to_string(),
              new_version: info.version,
              new_reference,
            })),
            Err(err) => {
              // output and fallback to using the info file
              log_warn!(environment, "Failed reading plugin latest info. {:#}", err);
              None
            }
          }
        }
        Err(err) => {
          log_warn!(environment, "Failed reading plugin latest info. {:#}", err);
          None
        }
      }
    } else {
      log_warn!(
        environment,
        "Skipping {} as it did not specify an update url. Please update manually.",
        plugin.info().name
      );
      None
    }
  }

  let config_file_plugins = get_config_file_plugins(plugin_resolver, plugins).await;
  // run each plugin's latest-info lookup in parallel — the network round-trip
  // dominates and serializing them multiplies latency by the plugin count
  let tasks = config_file_plugins
    .into_iter()
    .map(|(plugin_reference, plugin_result)| {
      let environment = environment.clone();
      dprint_core::async_runtime::spawn(async move { resolve_plugin_update_info(&environment, plugin_reference, plugin_result).await })
    })
    .collect::<Vec<_>>();

  let mut final_infos = Vec::with_capacity(tasks.len());
  for result in future::join_all(tasks).await {
    let maybe_info = result.unwrap();
    if let Some(info) = maybe_info
      && info.as_ref().ok().map(|info| info.old_version != info.new_version).unwrap_or(true)
    {
      final_infos.push(info);
    }
  }
  Ok(final_infos)
}

pub async fn output_resolved_config<TEnvironment: Environment>(
  cmd: &OutputResolvedConfigSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config = Rc::new(resolve_config_from_args(args, environment).await?);
  let plugins_scope = resolve_plugins_scope(config, environment, plugin_resolver).await?;
  plugins_scope.ensure_no_global_config_diagnostics()?;

  // when a file path is provided, limit the output to only the plugins that
  // would format that file (resolving associations, file names, and extensions
  // the same way `dprint fmt` does). this helps debug why a plugin isn't
  // running on a file (ex. a missing `associations` entry — see issue #794).
  let included_plugin_names = cmd.file_path.as_ref().map(|file_path| {
    let file_path = environment.cwd().join(file_path);
    plugins_scope
      .plugin_name_maps
      .get_plugin_names_from_file_path(&file_path)
      .into_iter()
      .collect::<HashSet<_>>()
  });

  let mut plugin_jsons = Vec::new();
  for plugin in plugins_scope.plugins.values() {
    if let Some(included_plugin_names) = &included_plugin_names
      && !included_plugin_names.contains(plugin.name())
    {
      continue;
    }
    let config_key = &plugin.info().config_key;

    // output its diagnostics
    let plugin = match plugin.get_or_create_checking_config_diagnostics(environment).await? {
      GetPluginResult::HadDiagnostics(count) => bail!("Plugin had {} diagnostic(s)", count),
      GetPluginResult::Success(plugin) => plugin,
    };

    let text = plugin.resolved_config().await?;
    let pretty_text = pretty_print_json_text(&text)?;
    plugin_jsons.push(format!("\"{}\": {}", config_key, pretty_text));
  }

  environment.log_machine_readable(
    &if plugin_jsons.is_empty() {
      "{}".to_string()
    } else {
      let text = plugin_jsons.join(",\n").lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n");
      format!("{{\n{}\n}}", text)
    }
    .into_bytes(),
  );

  Ok(())
}

async fn get_config_file_plugins<TEnvironment: Environment>(
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  current_plugins: Vec<PluginSourceReference>,
) -> Vec<(PluginSourceReference, Result<Rc<PluginWrapper>>)> {
  let tasks = current_plugins
    .into_iter()
    .map(|plugin_reference| {
      let plugin_resolver = plugin_resolver.clone();
      dprint_core::async_runtime::spawn(async move {
        let resolve_result = plugin_resolver.resolve_plugin(plugin_reference.clone()).await;
        (plugin_reference, resolve_result)
      })
    })
    .collect::<Vec<_>>();

  let mut results = Vec::with_capacity(tasks.len());
  for result in future::join_all(tasks).await {
    results.push(result.unwrap());
  }
  results
}

fn select_editor_args(env: &impl Environment) -> Vec<String> {
  fn try_parse_env_var(env: &impl Environment, name: &str) -> Option<Vec<String>> {
    let var = env.env_var(name).filter(|v| !v.is_empty()).and_then(|v| v.into_string().ok())?;
    match crate::utils::parse_command_line(&var) {
      Ok(value) => Some(value),
      Err(err) => {
        log_warn!(env, "Failed resolving '{}' env var: {:#}", name, err);
        None
      }
    }
  }
  if let Some(value) = try_parse_env_var(env, "DPRINT_EDITOR") {
    return value;
  }
  if let Some(value) = try_parse_env_var(env, "VISUAL") {
    return value;
  }
  if let Some(value) = try_parse_env_var(env, "EDITOR") {
    return value;
  }
  if cfg!(windows) {
    Vec::from(["notepad".to_string()])
  } else {
    // I prefer vim, but this is probably more friendly for people
    Vec::from(["nano".to_string()])
  }
}

#[cfg(test)]
mod test {
  use std::path::Path;

  use anyhow::Result;
  use deno_terminal::colors;
  use once_cell::sync::Lazy;
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use crate::assert_contains;
  use crate::configuration::*;
  use crate::environment::CanonicalizedPathBuf;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFilePlugin;
  use crate::test_helpers::TestProcessPluginFile;
  use crate::test_helpers::TestProcessPluginFileBuilder;
  use crate::test_helpers::get_test_wasm_plugin_checksum;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn should_initialize() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-typescript".to_string(),
            version: "0.17.2".to_string(),
            url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
            config_key: Some("typescript".to_string()),
            file_extensions: vec!["ts".to_string()],
            config_excludes: vec![],
            ..Default::default()
          })
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-jsonc".to_string(),
            version: "0.2.3".to_string(),
            url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
            config_key: Some("json".to_string()),
            file_extensions: vec!["json".to_string()],
            config_excludes: vec![],
            ..Default::default()
          });
      })
      .build();
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        "\nCreated dprint.json",
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
      ]
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn should_use_dprint_config_init_as_alias() {
    let environment = TestEnvironment::new();
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });

    run_test_cli(vec!["config", "init"], &environment).unwrap();
    environment.take_stderr_messages();
    environment.take_stdout_messages();
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn should_initialize_with_specified_config_path() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec![],
          ..Default::default()
        });
      })
      .build();
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init", "--config", "./test.config.json"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        "\nCreated ./test.config.json",
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
      ]
    );
    assert_eq!(environment.read_file("./test.config.json").unwrap(), expected_text);
  }

  #[test]
  fn should_error_when_config_file_exists_on_initialize() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|c| {
        c.add_includes("**/*.txt");
      })
      .build();
    let error_message = run_test_cli(vec!["init"], &environment).err().unwrap();
    assert_eq!(error_message.to_string(), "Configuration file 'dprint.json' already exists.");
  }

  #[test]
  fn should_initialize_global_config() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-typescript".to_string(),
            version: "0.17.2".to_string(),
            url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
            config_key: Some("typescript".to_string()),
            file_extensions: vec!["ts".to_string()],
            config_excludes: vec![],
            ..Default::default()
          })
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-jsonc".to_string(),
            version: "0.2.3".to_string(),
            url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
            config_key: Some("json".to_string()),
            file_extensions: vec!["json".to_string()],
            config_excludes: vec![],
            ..Default::default()
          });
      })
      .build();
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init", "--global"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    let config_path = if std::env::consts::OS == "macos" {
      Path::new("/home/.config/dprint")
    } else {
      Path::new("/config/dprint")
    }
    .join("dprint.jsonc");
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        format!("\nCreated {}", config_path.display()),
        "\nRun `dprint config edit --global` to modify this file in the future.".to_string(),
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor".to_string()
      ]
    );
    assert_eq!(environment.read_file(config_path).unwrap(), expected_text);
  }

  #[test]
  fn should_initialize_global_config_via_env_var() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec![],
          ..Default::default()
        });
      })
      .build();
    environment.set_env_var("DPRINT_CONFIG_DIR", Some("/custom/config"));
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init", "--global"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        format!("\nCreated {}", Path::new("/custom/config").join("dprint.jsonc").display()),
        "\nRun `dprint config edit --global` to modify this file in the future.".to_string(),
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor".to_string()
      ]
    );
    assert_eq!(environment.read_file("/custom/config/dprint.jsonc").unwrap(), expected_text);
  }

  #[test]
  fn should_error_when_global_config_file_already_exists() {
    let environment = TestEnvironmentBuilder::new().write_file("/config/dprint/dprint.json", "{}").build();
    let error_message = run_test_cli(vec!["config", "init", "--global"], &environment).err().unwrap();
    assert_eq!(
      error_message.to_string(),
      format!(
        "Configuration file '{}' already exists.",
        Path::new("/config/dprint").join("dprint.json").display()
      )
    );
  }

  #[test]
  fn config_add() {
    let old_wasm_url = "https://plugins.dprint.dev/test-plugin-0.1.0.wasm".to_string();
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    let old_ps_checksum = OLD_PROCESS_PLUGIN_FILE.checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.json@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    let select_plugin_msg = "Select a plugin to add:".to_string();

    // no plugins specified
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      selection_result: Some(0),
    });

    // process plugin specified
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: true,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      selection_result: Some(1),
    });

    // process plugin specified no checksum in info
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_ps_url.clone()],
      selection_result: Some(1),
    });

    // wasm exists, no process
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: true,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![old_wasm_url.clone(), new_ps_url.clone()],
      selection_result: Some(0),
    });

    // process exists, no wasm
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: true,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![old_ps_url.clone(), new_wasm_url.clone()],
      selection_result: Some(0),
    });

    // all plugins already specified in config
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: true,
      config_has_process: true,
      remote_has_checksums: false,
      expected_error: Some("Could not find any plugins to add. Please provide one by specifying `dprint add <plugin-url>`."),
      expected_logs: vec![],
      expected_urls: vec![],
      selection_result: Some(0),
    });

    // using arg
    test_add(TestAddOptions {
      add_arg: Some("test-plugin"),
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![new_wasm_url.clone()],
      selection_result: None,
    });

    // using arg and no existing plugin
    test_add(TestAddOptions {
      add_arg: Some("my-plugin"),
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: Some(
        "Could not find plugin with name 'my-plugin'. Please fix the name or try a url instead.\n\nPlugins:\n * test-plugin\n * test-process-plugin",
      ),
      expected_logs: vec![],
      expected_urls: vec![],
      selection_result: None,
    });

    // using and already exists
    test_add(TestAddOptions {
      add_arg: Some("test-plugin"),
      config_has_wasm: true,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![
        // upgrades to the latest
        new_wasm_url,
      ],
      selection_result: None,
    });

    // using url
    test_add(TestAddOptions {
      add_arg: Some("https://plugins.dprint.dev/my-plugin.wasm"),
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec!["https://plugins.dprint.dev/my-plugin.wasm".to_string()],
      selection_result: None,
    });
  }

  #[derive(Debug)]
  struct TestAddOptions {
    add_arg: Option<&'static str>,
    config_has_wasm: bool,
    config_has_process: bool,
    remote_has_checksums: bool,
    selection_result: Option<usize>,
    expected_error: Option<&'static str>,
    expected_logs: Vec<String>,
    expected_urls: Vec<String>,
  }

  #[track_caller]
  fn test_add(options: TestAddOptions) {
    let expected_logs = options.expected_logs.clone();
    let expected_urls = options.expected_urls.clone();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: options.config_has_wasm,
      config_has_wasm_checksum: false,
      config_has_process: options.config_has_process,
      remote_has_wasm_checksum: options.remote_has_checksums,
      remote_has_process_checksum: options.remote_has_checksums,
    });
    if let Some(selection_result) = options.selection_result {
      environment.set_selection_result(selection_result);
    }
    let mut args = vec!["config", "add"];
    if let Some(add_arg) = options.add_arg {
      args.push(add_arg);
    }
    match run_test_cli(args, &environment) {
      Ok(()) => {
        assert!(options.expected_error.is_none());
      }
      Err(err) => {
        assert_eq!(Some(err.to_string()), options.expected_error.map(ToOwned::to_owned));
      }
    }
    assert_eq!(environment.take_stderr_messages(), expected_logs);

    if options.expected_error.is_none() {
      let expected_text = format!(
        r#"{{
  "plugins": [
{}
  ]
}}"#,
        expected_urls.into_iter().map(|u| format!("    \"{}\"", u)).collect::<Vec<_>>().join(",\n")
      );
      assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
    }
  }

  #[test]
  fn config_add_multiple() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm";
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json";
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
    });
    run_test_cli(vec!["add", "test-plugin", "test-process-plugin"], &environment).unwrap();
    let expected_text = format!(
      r#"{{
  "plugins": [
    "{}",
    "{}"
  ]
}}"#,
      new_wasm_url, new_ps_url,
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn config_add_checksum_named_wasm_plugin() {
    // `--checksum` makes a named wasm add carry the registry's checksum,
    // which is otherwise omitted for wasm plugins.
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: false,
    });
    run_test_cli(vec!["config", "add", "--checksum", "test-plugin"], &environment).unwrap();
    let expected = format!("https://plugins.dprint.dev/test-plugin.wasm@{}", get_test_wasm_plugin_checksum());
    let dprint_json = environment.read_file("./dprint.json").unwrap();
    assert!(dprint_json.contains(&expected), "got: {dprint_json}");
    let _ = environment.take_stderr_messages();
  }

  #[test]
  fn config_add_global() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
    });
    // Create a global config file
    environment.mk_dir_all("/config/dprint").unwrap();
    environment
      .write_file(
        "/config/dprint/dprint.json",
        r#"{
  "plugins": [
  ]
}"#,
      )
      .unwrap();

    // Test adding a plugin by name to the global config
    run_test_cli(vec!["config", "add", "--global", "test-plugin"], &environment).unwrap();

    let expected_text = format!(
      r#"{{
  "plugins": [
    "{}"
  ]
}}"#,
      new_wasm_url
    );
    assert_eq!(environment.read_file("/config/dprint/dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn config_update_global() {
    let old_wasm_url = "https://plugins.dprint.dev/test-plugin-0.1.0.wasm".to_string();
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
    });
    // Create a global config file with an old plugin version
    environment.mk_dir_all("/config/dprint").unwrap();
    environment
      .write_file(
        "/config/dprint/dprint.json",
        &format!(
          r#"{{
  "plugins": [
    "{}"
  ]
}}"#,
          old_wasm_url
        ),
      )
      .unwrap();

    // Test updating the plugin in the global config
    run_test_cli(vec!["config", "update", "--global"], &environment).unwrap();

    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
      ]
    );

    let expected_text = format!(
      r#"{{
  "plugins": [
    "{}"
  ]
}}"#,
      new_wasm_url
    );
    assert_eq!(environment.read_file("/config/dprint/dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn config_update_should_always_upgrade_to_latest_plugins() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    // test all the process plugin combinations
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: Vec::new(),
      expected_logs: vec!["Updating test-process-plugin 0.1.0 to 0.3.0...".to_string()],
      expected_urls: vec![new_ps_url.clone()],
      always_update: true,
      on_error: Some(Box::new(|text| {
        assert_contains!(
          text,
          "Error resolving plugin https://plugins.dprint.dev/test-plugin-3.json: The plugin must have a checksum specified for security reasons since it is not a Wasm plugin."
        );
      })),
      exit_code: 12,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });

    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_wasm_url.clone(), new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });
  }

  #[test]
  fn config_update_should_upgrade_to_latest_plugins() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    let new_wasm_url_with_checksum = format!("{}@{}", new_wasm_url, get_test_wasm_plugin_checksum());
    let updating_message = "Updating test-plugin 0.1.0 to 0.2.0...".to_string();
    let compiling_message = "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string();

    // test all the wasm combinations
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url_with_checksum.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });

    // test all the process plugin combinations
    let old_ps_checksum = TestProcessPluginFile::default().checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.json@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url_with_checksum),
        "Do you want to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you want to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
      ],
      expected_urls: vec![new_ps_url.clone()],
      always_update: false,
      on_error: Some(Box::new(|text| {
        assert_contains!(
          text,
          "Error resolving plugin https://plugins.dprint.dev/test-plugin-3.json: The plugin must have a checksum specified for security reasons since it is not a Wasm plugin."
        );
      })),
      exit_code: 12,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: vec![Ok(None)],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you want to update it? N".to_string(),
      ],
      expected_urls: vec![old_ps_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });

    // testing both in config, but only updating one
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: vec![Ok(Some(false))],
      expected_logs: vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url_with_checksum),
        "Do you want to update it? N".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
      ],
      expected_urls: vec![new_wasm_url.clone(), old_ps_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
  }

  #[test]
  fn config_update_plugin_config() {
    let mut builder = get_setup_builder(SetupEnvOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
    });
    builder.with_default_config(|config| {
      config.add_config_section(
        "testProcessPlugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
      config.add_config_section(
        "test-plugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
    });
    builder.with_local_config("/sub_folder/dprint.json", |config| {
      config
        .add_remote_process_plugin()
        .add_remote_wasm_plugin_0_1_0()
        .add_config_section(
          "testProcessPlugin",
          r#"{
  "should_set": "asdf"
}"#,
        )
        .add_config_section(
          "test-plugin",
          r#"{
  "should_set": "asdf"
}"#,
        );
    });
    let environment = builder.initialize().build();
    run_test_cli(vec!["config", "update", "--yes", "--recursive"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Updating test-process-plugin 0.1.0 in /sub_folder/dprint.json to 0.3.0...".to_string(),
        "Updating test-plugin 0.1.0 in /sub_folder/dprint.json to 0.2.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string()
      ]
    );
    assert_eq!(
      environment.read_file("./dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_add": "new_value",
    "should_set": "new_value",
    "should_set_past_version": "0.1.0",
    "new_prop1": ["new_value"],
    "new_prop2": {{
      "new_prop": "new_value"
    }}
  }},
  "test-plugin": {{
    "should_add": "new_value_wasm",
    "should_set": "new_value_wasm",
    "should_set_past_version": "0.1.0",
    "new_prop1": ["new_value_wasm"],
    "new_prop2": {{
      "new_prop": "new_value_wasm"
    }}
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-plugin.wasm",
    "https://plugins.dprint.dev/test-plugin-3.json@{}"
  ]
}}"#,
        NEW_PROCESS_PLUGIN_FILE.checksum()
      )
    );
    assert_eq!(
      environment.read_file("./sub_folder/dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_set": "new_value"
  }},
  "test-plugin": {{
    "should_set": "new_value_wasm"
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-plugin-3.json@{}",
    "https://plugins.dprint.dev/test-plugin.wasm"
  ]
}}"#,
        NEW_PROCESS_PLUGIN_FILE.checksum()
      )
    );
  }

  #[test]
  fn config_update_default_non_recursive() {
    let mut builder = get_setup_builder(SetupEnvOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
    });
    builder.with_default_config(|config| {
      config.add_config_section(
        "testProcessPlugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
      config.add_config_section(
        "test-plugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
    });
    builder.with_local_config("/sub_folder/dprint.json", |config| {
      config
        .add_remote_process_plugin()
        .add_remote_wasm_plugin_0_1_0()
        .add_config_section(
          "testProcessPlugin",
          r#"{
  "should_set": "asdf"
}"#,
        )
        .add_config_section(
          "test-plugin",
          r#"{
  "should_set": "asdf"
}"#,
        );
    });
    let environment = builder.initialize().build();
    // Without --recursive, should only update the root config
    run_test_cli(vec!["config", "update", "--yes"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string()
      ]
    );
    // Verify the root config was updated
    assert_eq!(
      environment.read_file("./dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_add": "new_value",
    "should_set": "new_value",
    "should_set_past_version": "0.1.0",
    "new_prop1": ["new_value"],
    "new_prop2": {{
      "new_prop": "new_value"
    }}
  }},
  "test-plugin": {{
    "should_add": "new_value_wasm",
    "should_set": "new_value_wasm",
    "should_set_past_version": "0.1.0",
    "new_prop1": ["new_value_wasm"],
    "new_prop2": {{
      "new_prop": "new_value_wasm"
    }}
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-plugin.wasm",
    "https://plugins.dprint.dev/test-plugin-3.json@{}"
  ]
}}"#,
        NEW_PROCESS_PLUGIN_FILE.checksum()
      )
    );
    // Verify the sub_folder config was NOT updated (should still have old URLs)
    let old_ps_checksum = TestProcessPluginFile::default().checksum();
    assert_eq!(
      environment.read_file("./sub_folder/dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_set": "asdf"
  }},
  "test-plugin": {{
    "should_set": "asdf"
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-process.json@{}",
    "https://plugins.dprint.dev/test-plugin-0.1.0.wasm"
  ]
}}"#,
        old_ps_checksum
      )
    );
  }

  #[test]
  fn config_update_dry_run_does_not_modify_files() {
    let mut builder = get_setup_builder(SetupEnvOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
    });
    builder.with_default_config(|config| {
      config.add_config_section(
        "testProcessPlugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
      config.add_config_section(
        "test-plugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
  "should_set_past_version": ""
}"#,
      );
    });
    builder.with_local_config("/sub_folder/dprint.json", |config| {
      config
        .add_remote_process_plugin()
        .add_remote_wasm_plugin_0_1_0()
        .add_config_section(
          "testProcessPlugin",
          r#"{
  "should_set": "asdf"
}"#,
        )
        .add_config_section(
          "test-plugin",
          r#"{
  "should_set": "asdf"
}"#,
        );
    });
    let environment = builder.initialize().build();

    let root_before = environment.read_file("./dprint.json").unwrap();
    let sub_before = environment.read_file("./sub_folder/dprint.json").unwrap();

    run_test_cli(vec!["config", "update", "--recursive", "--dry-run"], &environment).unwrap();

    // it should report what would change, but make no edits to the files
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        format!("Would update {} 0.1.0 to 0.2.0.", colors::bold("test-plugin")),
        format!("Would update {} 0.1.0 to 0.3.0.", colors::bold("test-process-plugin")),
        format!(
          "Would update {} 0.1.0 in /sub_folder/dprint.json to 0.3.0.",
          colors::bold("test-process-plugin")
        ),
        format!("Would update {} 0.1.0 in /sub_folder/dprint.json to 0.2.0.", colors::bold("test-plugin")),
        "Compiling https://plugins.dprint.dev/test-plugin.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
        format!("\n{}", colors::gray("This was a dry run. No files were changed.")),
      ]
    );

    // the preview output shows the would-be config (with migrated sections and bumped plugin urls)
    let stdout = environment.take_stdout_messages().join("\n");
    assert_contains!(stdout, &colors::bold("/dprint.json would be updated to:").to_string());
    assert_contains!(stdout, "\"should_add\": \"new_value\"");
    assert_contains!(stdout, "\"should_add\": \"new_value_wasm\"");
    assert_contains!(stdout, "https://plugins.dprint.dev/test-plugin.wasm");
    assert_contains!(
      stdout,
      &format!("https://plugins.dprint.dev/test-plugin-3.json@{}", NEW_PROCESS_PLUGIN_FILE.checksum())
    );

    // the files on disk must be untouched
    assert_eq!(environment.read_file("./dprint.json").unwrap(), root_before);
    assert_eq!(environment.read_file("./sub_folder/dprint.json").unwrap(), sub_before);
  }

  struct TestUpdateOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    remote_has_wasm_checksum: bool,
    remote_has_process_checksum: bool,
    confirm_results: Vec<Result<Option<bool>>>,
    expected_logs: Vec<String>,
    expected_urls: Vec<String>,
    always_update: bool,
    on_error: Option<Box<dyn FnOnce(&str)>>,
    exit_code: i32,
  }

  #[track_caller]
  fn test_update(options: TestUpdateOptions) {
    let expected_logs = options.expected_logs.clone();
    let expected_urls = options.expected_urls.clone();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: options.config_has_wasm,
      config_has_wasm_checksum: options.config_has_wasm_checksum,
      config_has_process: options.config_has_process,
      remote_has_wasm_checksum: options.remote_has_wasm_checksum,
      remote_has_process_checksum: options.remote_has_process_checksum,
    });
    environment.set_confirm_results(options.confirm_results);

    let result = run_test_cli(
      if options.always_update {
        vec!["config", "update", "--yes"]
      } else {
        vec!["config", "update"]
      },
      &environment,
    );
    if let Err(err) = result {
      let on_error = match options.on_error {
        Some(on_error) => on_error,
        None => panic!("{:#}", err),
      };
      (on_error)(&err.to_string());
      err.assert_exit_code(options.exit_code);
    } else {
      assert_eq!(options.on_error.is_some(), false);
      assert_eq!(options.exit_code, 0);
    }
    assert_eq!(environment.take_stderr_messages(), expected_logs);

    let expected_text = format!(
      r#"{{
  "plugins": [
{}
  ]
}}"#,
      expected_urls.into_iter().map(|u| format!("    \"{}\"", u)).collect::<Vec<_>>().join(",\n")
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  static OLD_PROCESS_PLUGIN_FILE: Lazy<TestProcessPluginFile> = Lazy::new(|| TestProcessPluginFileBuilder::default().version("0.1.0").build());
  static NEW_PROCESS_PLUGIN_FILE: Lazy<TestProcessPluginFile> = Lazy::new(|| TestProcessPluginFileBuilder::default().version("0.3.0").build());

  #[derive(Debug)]
  struct SetupEnvOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    remote_has_wasm_checksum: bool,
    remote_has_process_checksum: bool,
  }

  fn get_setup_env(opts: SetupEnvOptions) -> TestEnvironment {
    get_setup_builder(opts).initialize().build()
  }

  fn get_setup_builder(opts: SetupEnvOptions) -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();

    if opts.config_has_wasm {
      builder.add_remote_wasm_plugin();
      builder.add_remote_wasm_0_1_0_plugin();
    }
    if opts.config_has_process {
      builder.add_remote_process_plugin();
      builder.add_remote_process_plugin_at_url("https://plugins.dprint.dev/test-plugin-3.json", &*NEW_PROCESS_PLUGIN_FILE);
    }

    builder
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "test-plugin".to_string(),
          version: "0.2.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin.wasm".to_string(),
          config_key: Some("test-plugin".to_string()),
          checksum: if opts.remote_has_wasm_checksum {
            Some(get_test_wasm_plugin_checksum())
          } else {
            None
          },
          ..Default::default()
        });

        info.add_plugin(TestInfoFilePlugin {
          name: "test-process-plugin".to_string(),
          version: "0.3.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin-3.json".to_string(),
          config_key: Some("test-process-plugin".to_string()),
          checksum: if opts.remote_has_process_checksum {
            Some(NEW_PROCESS_PLUGIN_FILE.checksum())
          } else {
            None
          },
          ..Default::default()
        });
      })
      .with_default_config(|config| {
        config.ensure_plugins_section();
        if opts.config_has_wasm {
          if opts.config_has_wasm_checksum {
            config.add_remote_wasm_plugin_0_1_0_with_checksum();
          } else {
            config.add_remote_wasm_plugin_0_1_0();
          }
        }
        if opts.config_has_process {
          // this will add it with the checksum
          // Don't bother testing this without a checksum because it won't resolve the plugin
          config.add_remote_process_plugin();
        }
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin.wasm",
          "version": "0.2.0",
          "checksum": if opts.remote_has_wasm_checksum { Some(get_test_wasm_plugin_checksum()) } else { None },
        })
        .to_string(),
      )
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-process-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin-3.json",
          "version": "0.3.0",
          "checksum": if opts.remote_has_process_checksum { Some(NEW_PROCESS_PLUGIN_FILE.checksum()) } else { None },
        })
        .to_string(),
      );
    builder
  }

  #[test]
  fn config_update_should_not_upgrade_when_at_latest_plugins() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_info_file(|_| {})
      .with_default_config(|config| {
        config.add_remote_wasm_plugin();
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin.wasm",
          "version": "0.2.0"
        })
        .to_string(),
      )
      .initialize()
      .build();
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    // should be empty because nothing to upgrade
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn config_update_should_handle_wasm_to_process_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_wasm_0_1_0_plugin()
      .with_info_file(|_| {})
      .with_default_config(|config| {
        config.add_remote_wasm_plugin_0_1_0();
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin.json",
          "version": "0.2.0",
          "checksum": "checksum",
        })
        .to_string(),
      )
      .initialize()
      .build();
    environment.set_confirm_results(vec![Ok(None)]);
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "The process plugin test-plugin 0.1.0 has a new url: https://plugins.dprint.dev/test-plugin.json@checksum",
        "Do you want to update it? N"
      ]
    );
  }

  #[test]
  fn should_output_resolved_config() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![concat!(
        "{\n",
        "  \"test-plugin\": {\n",
        "    \"ending\": \"formatted\",\n",
        "    \"lineWidth\": 120\n",
        "  },\n",
        "  \"testProcessPlugin\": {\n",
        "    \"ending\": \"formatted_process\",\n",
        "    \"lineWidth\": 120\n",
        "  }\n",
        "}",
      )]
    );
  }

  #[test]
  fn should_output_resolved_config_for_file_path() {
    // the wasm plugin formats `.txt`, the process plugin formats `.txt_ps` —
    // passing a file path limits the output to the plugin that handles it
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["resolved-config", "--file", "file.txt"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![concat!(
        "{\n",
        "  \"test-plugin\": {\n",
        "    \"ending\": \"formatted\",\n",
        "    \"lineWidth\": 120\n",
        "  }\n",
        "}",
      )]
    );

    run_test_cli(vec!["resolved-config", "--file", "file.txt_ps"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![concat!(
        "{\n",
        "  \"testProcessPlugin\": {\n",
        "    \"ending\": \"formatted_process\",\n",
        "    \"lineWidth\": 120\n",
        "  }\n",
        "}",
      )]
    );
  }

  #[test]
  fn should_output_empty_resolved_config_for_unhandled_file_path() {
    // a file no plugin handles outputs an empty object rather than every plugin
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["resolved-config", "--file", "file.unknown_ext"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["{}"]);
  }

  #[test]
  fn should_output_resolved_config_for_file_path_with_associations() {
    // associations are additive, so the plugin keeps matching its default `.txt`
    // extension and also matches the associated `.special` files (issues #794, #841)
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
            "associations": ["**/*.special"]
          }"#,
        );
      })
      .build();

    let expected_handled = vec![concat!(
      "{\n",
      "  \"test-plugin\": {\n",
      "    \"ending\": \"formatted\",\n",
      "    \"lineWidth\": 120\n",
      "  }\n",
      "}",
    )];

    // a file matching neither the defaults nor the associations is unhandled
    run_test_cli(vec!["resolved-config", "--file", "file.asdf"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["{}"]);

    // the plugin still formats its default `.txt` extension
    run_test_cli(vec!["resolved-config", "--file", "file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), expected_handled.clone());

    // and also the associated `.special` files
    run_test_cli(vec!["resolved-config", "--file", "file.special"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), expected_handled);
  }

  #[test]
  fn should_output_base_resolved_config_when_overrides_exist() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
            "ending": "base",
            "overrides": {
              "files": "**/package.json",
              "ending": "package"
            }
          }"#,
        );
      })
      .build();

    run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![concat!(
        "{\n",
        "  \"test-plugin\": {\n",
        "    \"ending\": \"base\",\n",
        "    \"lineWidth\": 120\n",
        "  }\n",
        "}",
      )]
    );
  }

  #[test]
  fn should_error_for_override_config_diagnostics() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
            "overrides": {
              "files": "**/package.json",
              "unknownProperty": true
            }
          }"#,
        );
      })
      .build();

    let err = run_test_cli(vec!["output-resolved-config"], &environment).err().unwrap();
    assert_eq!(err.to_string(), "Plugin had 1 diagnostic(s)");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "[test-plugin]: Unknown property in configuration (unknownProperty)",
        "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
      ]
    );
  }

  #[test]
  fn should_output_resolved_config_no_plugins() {
    let environment = TestEnvironmentBuilder::new().with_default_config(|_| {}).build();
    run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["{}"]);
  }

  #[test]
  fn config_edit_should_open_editor_with_local_config() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    assert_eq!(commands.len(), 1);
    let (args, _) = &commands[0];

    // Should use the default editor (nano on non-Windows)
    #[cfg(not(windows))]
    {
      assert_eq!(args.len(), 2);
      assert_eq!(args[0], "nano");
      assert_eq!(args[1], "/dprint.json");
    }

    #[cfg(windows)]
    {
      assert_eq!(args.len(), 2);
      assert_eq!(args[0], "notepad");
      assert_eq!(args[1], "/dprint.json");
    }
  }

  #[test]
  fn config_edit_should_use_dprint_editor_env_var() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_env_var("DPRINT_EDITOR", Some("vim"));
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    assert_eq!(commands.len(), 1);
    let (args, _) = &commands[0];
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "vim");
    assert_eq!(args[1], "/dprint.json");
  }

  #[test]
  fn config_edit_should_use_visual_env_var() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_env_var("VISUAL", Some("emacs"));
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    assert_eq!(commands.len(), 1);
    let (args, _) = &commands[0];
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "emacs");
    assert_eq!(args[1], "/dprint.json");
  }

  #[test]
  fn config_edit_should_use_editor_env_var() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_env_var("EDITOR", Some("vi"));
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    assert_eq!(commands.len(), 1);
    let (args, _) = &commands[0];
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "vi");
    assert_eq!(args[1], "/dprint.json");
  }

  #[test]
  fn config_edit_should_prioritize_dprint_editor_over_others() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_env_var("DPRINT_EDITOR", Some("vim"));
    environment.set_env_var("VISUAL", Some("emacs"));
    environment.set_env_var("EDITOR", Some("vi"));
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    let (args, _) = &commands[0];
    assert_eq!(args[0], "vim");
  }

  #[test]
  fn config_edit_should_handle_editor_with_args() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_env_var("DPRINT_EDITOR", Some("code --wait"));
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit"], &environment).unwrap();

    let commands = environment.take_run_commands();
    let (args, _) = &commands[0];
    assert_eq!(args.len(), 3);
    assert_eq!(args[0], "code");
    assert_eq!(args[1], "--wait");
    assert_eq!(args[2], "/dprint.json");
  }

  #[test]
  fn config_edit_should_open_global_config() {
    let environment = TestEnvironmentBuilder::new().write_file("/config/dprint/dprint.json", "{}").build();
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit", "--global"], &environment).unwrap();

    let commands = environment.take_run_commands();
    let (args, _) = &commands[0];

    assert_eq!(
      args[args.len() - 1].to_string_lossy(),
      Path::new("/config/dprint").join("dprint.json").to_string_lossy()
    );
  }

  #[test]
  fn config_edit_should_error_when_no_config_found() {
    let environment = TestEnvironment::new();

    let error = run_test_cli(vec!["config", "edit"], &environment).err().unwrap();
    assert_eq!(error.to_string(), "Could not find a configuration file. Create one with `dprint init`");
  }

  #[test]
  fn config_edit_should_error_when_no_global_config_found() {
    let environment = TestEnvironment::new();

    let error = run_test_cli(vec!["config", "edit", "--global"], &environment).err().unwrap();
    assert_eq!(
      error.to_string(),
      "Could not find global dprint.json file. Create one with `dprint init --global`"
    );
  }

  #[test]
  fn config_edit_should_error_on_remote_config() {
    let environment = TestEnvironmentBuilder::new()
      .with_remote_config("https://example.com/dprint.json", |config| {
        config.add_includes("**/*.txt");
      })
      .build();

    let error = run_test_cli(vec!["config", "edit", "-c", "https://example.com/dprint.json"], &environment)
      .err()
      .unwrap();
    assert_eq!(error.to_string(), "Cannot edit a remote configuration file 'https://example.com/dprint.json'");
  }

  #[test]
  fn config_edit_should_error_when_editor_exits_with_non_zero() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_run_command_result(Ok(Some(1)));

    let error = run_test_cli(vec!["config", "edit"], &environment).err().unwrap();
    assert_eq!(error.to_string(), "Editor exited with code: 1");
  }

  #[test]
  fn config_edit_should_work_with_custom_config_path() {
    let environment = TestEnvironmentBuilder::new()
      .with_local_config("custom.config.json", |config| {
        config.add_includes("**/*.txt");
      })
      .build();
    environment.set_run_command_result(Ok(Some(0)));

    run_test_cli(vec!["config", "edit", "-c", "custom.config.json"], &environment).unwrap();

    let commands = environment.take_run_commands();
    let (args, _) = &commands[0];
    assert_eq!(args[args.len() - 1], "/custom.config.json");
  }

  /// Convenience for tests: most calls only vary the spec text and the
  /// two flags, so wrap the struct-building boilerplate here.
  async fn call_resolve_npm_plugin_to_add(
    text: &str,
    config_path: &CanonicalizedPathBuf,
    no_version: bool,
    update_package_json: bool,
    environment: &TestEnvironment,
  ) -> Result<super::ResolvedNpmPluginAdd> {
    call_resolve_npm_plugin_to_add_checksum(text, config_path, no_version, update_package_json, false, environment).await
  }

  /// Like [`call_resolve_npm_plugin_to_add`] but lets a test set `--checksum`.
  async fn call_resolve_npm_plugin_to_add_checksum(
    text: &str,
    config_path: &CanonicalizedPathBuf,
    no_version: bool,
    update_package_json: bool,
    checksum: bool,
    environment: &TestEnvironment,
  ) -> Result<super::ResolvedNpmPluginAdd> {
    super::resolve_npm_plugin_to_add(
      super::ResolveNpmPluginOptions {
        text,
        config_path,
        no_version,
        update_package_json,
        checksum,
      },
      environment,
    )
    .await
  }

  #[tokio::test]
  async fn npm_add_pinned_wasm_keeps_bare_form() {
    // a pinned version without a path inspects that version's tarball to learn
    // the kind; a wasm package keeps the bare `name@version` form.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "0.95.15" },
      "versions": { "0.95.15": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.95.15.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/typescript", packument.to_string().into_bytes());
    environment.add_remote_file_bytes(
      "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.95.15.tgz",
      create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]),
    );
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript@0.95.15", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript@0.95.15");
    assert!(result.package_json_addition.is_none());
  }

  #[tokio::test]
  async fn npm_add_pinned_with_explicit_path_passes_through() {
    // an explicit plugin file is taken at face value and passed through
    // verbatim without touching the registry.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript@0.95.15/plugin.wasm", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript@0.95.15/plugin.wasm");
    assert!(result.package_json_addition.is_none());
  }

  #[tokio::test]
  async fn npm_add_checksum_forces_wasm_checksum_for_unversioned() {
    // `--checksum` makes an otherwise checksum-free wasm add carry one.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add_checksum("npm:foo", &config_path, false, false, true, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@1.2.3@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_checksum_forces_wasm_checksum_for_versioned() {
    // a pinned wasm add that would otherwise pass through verbatim gets a
    // checksum appended.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "9.9.9" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add_checksum("npm:foo@1.2.3", &config_path, false, false, true, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@1.2.3@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_checksum_explicit_non_root_path_does_not_inspect_package() {
    // an explicit plugin path that isn't at the tarball root must not trigger
    // kind detection (which only looks for a root plugin.wasm/plugin.json and
    // would bail). We just download to compute the checksum.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.0.0" },
      "versions": { "1.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    // tarball ships the plugin in a subdirectory, with no root plugin file
    let tarball_bytes = create_test_npm_tarball(&[("package/sub/foo.wasm", b"\0asm")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.0.0.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add_checksum("npm:foo@1.0.0/sub/foo.wasm", &config_path, false, false, true, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@1.0.0/sub/foo.wasm@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_checksum_pins_instead_of_deferring_to_devdep() {
    // `--checksum` requires a pinned version, so it overrides the usual
    // deferral to an unversioned spec when the package is in package.json.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment.write_file("/package.json", r#"{"devDependencies": {"foo": "^1.0.0"}}"#).unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add_checksum("npm:foo", &config_path, false, false, true, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@1.2.3@{}", expected));
  }

  #[tokio::test]
  async fn ensure_url_checksum_appends_for_plugin_url_without_one() {
    let environment = TestEnvironment::new();
    let bytes = vec![1u8, 2, 3, 4];
    let expected = crate::utils::get_sha256_checksum(&bytes);
    environment.add_remote_file_bytes("https://example.com/plugin.wasm", bytes.clone());
    let (url, prefetched) = super::ensure_url_checksum("https://example.com/plugin.wasm".to_string(), &environment)
      .await
      .unwrap();
    assert_eq!(url, format!("https://example.com/plugin.wasm@{}", expected));
    // the downloaded bytes are returned so the caller can warm the cache
    assert_eq!(prefetched, Some(bytes));
  }

  #[tokio::test]
  async fn ensure_url_checksum_preserves_existing_and_ignores_non_plugin_urls() {
    let environment = TestEnvironment::new();
    // already has a checksum → returned unchanged, no download attempted
    let (url, prefetched) = super::ensure_url_checksum("https://example.com/plugin.wasm@abc123".to_string(), &environment)
      .await
      .unwrap();
    assert_eq!(url, "https://example.com/plugin.wasm@abc123");
    assert_eq!(prefetched, None);
    // not a plugin file → returned unchanged
    let (url, prefetched) = super::ensure_url_checksum("https://example.com/readme.txt".to_string(), &environment)
      .await
      .unwrap();
    assert_eq!(url, "https://example.com/readme.txt");
    assert_eq!(prefetched, None);
  }

  #[tokio::test]
  async fn npm_add_defers_to_devdep_when_present() {
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment
      .write_file("/package.json", r#"{"devDependencies": {"@dprint/typescript": "^0.95.0"}}"#)
      .unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn npm_add_defers_to_regular_dependency_when_present() {
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment
      .write_file("/package.json", r#"{"dependencies": {"@dprint/typescript": "^0.95.0"}}"#)
      .unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn npm_add_defers_to_dep_listed_in_a_parent_package_json() {
    // monorepo layout: deps are listed at the workspace root, not in the
    // child workspace's package.json. Walk past the child package.json and
    // find the dep at the root.
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/repo/packages/web").unwrap();
    environment.write_file("/repo/packages/web/dprint.json", "{}").unwrap();
    // child package.json doesn't mention the plugin
    environment.write_file("/repo/packages/web/package.json", r#"{"name": "web"}"#).unwrap();
    // root package.json does
    environment
      .write_file("/repo/package.json", r#"{"devDependencies": {"@dprint/typescript": "^0.95.0"}}"#)
      .unwrap();

    let config_path = environment.canonicalize("/repo/packages/web/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn npm_add_resolves_latest_without_devdep() {
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    // detection downloads the tarball to learn the plugin kind — ship a wasm one
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:foo@1.2.3");
  }

  #[tokio::test]
  async fn npm_add_resolves_latest_with_checksum_for_process_plugin() {
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "2.0.0" },
      "versions": { "2.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-2.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = vec![9u8, 8, 7, 6];
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-2.0.0.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo/plugin.json", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@2.0.0/plugin.json@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_autodetects_process_plugin_without_path() {
    // `dprint add npm:foo` (no path) should inspect the package, discover it
    // ships a plugin.json, and write the pinned process form with a checksum.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "2.0.0" },
      "versions": { "2.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-2.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.json", b"{}")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-2.0.0.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@2.0.0/plugin.json@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_autodetects_wasm_plugin_without_path() {
    // `dprint add npm:foo` for a wasm package keeps the bare pinned form
    // (no checksum), even though we now download the tarball to detect.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "2.0.0" },
      "versions": { "2.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-2.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-2.0.0.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:foo@2.0.0");
  }

  #[tokio::test]
  async fn npm_add_autodetects_process_plugin_for_versioned_without_path() {
    // a pinned version without a path still inspects that version's tarball
    // rather than passing through verbatim as wasm.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "9.9.9" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = create_test_npm_tarball(&[("package/plugin.json", b"{}")]);
    let expected = crate::utils::get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo@1.2.3", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, format!("npm:foo@1.2.3/plugin.json@{}", expected));
  }

  #[tokio::test]
  async fn npm_add_defers_to_devdep_detects_process_path_from_node_modules() {
    // when deferring to an unversioned spec, detect from node_modules so a
    // process plugin still gets `/plugin.json`.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment.write_file("/package.json", r#"{"devDependencies": {"foo": "^1.0.0"}}"#).unwrap();
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", "{}").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, false, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:foo/plugin.json");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn npm_add_no_version_detects_process_path_from_node_modules() {
    // --no-version writes the unversioned form but should still pick up
    // `/plugin.json` for a process plugin installed in node_modules.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", "{}").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, true, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:foo/plugin.json");
  }

  #[tokio::test]
  async fn npm_add_no_version_skips_pinning_and_skips_registry() {
    // --no-version writes the unversioned spec without ever touching the
    // registry (the user explicitly asked us not to pin a version).
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    // intentionally no packument mock — verifies we don't fetch it
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript", &config_path, true, false, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript");
    assert!(result.package_json_addition.is_none());
  }

  #[tokio::test]
  async fn npm_add_no_version_errors_on_already_versioned_specifier() {
    // pinning is what --no-version turns off, so combining it with an
    // already-pinned specifier is a contradiction worth surfacing.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let err = call_resolve_npm_plugin_to_add("npm:@dprint/typescript@1.0.0", &config_path, true, false, &environment)
      .await
      .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("--no-version cannot be combined with a versioned specifier"), "got: {msg}");
  }

  #[tokio::test]
  async fn npm_add_package_json_returns_dev_dependency_with_caret_range() {
    // --package-json pulls dist-tags.latest, writes the unversioned spec
    // to dprint.json, and returns a caret-pinned devDependency entry the
    // caller queues for the package.json update.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "0.99.0" },
      "versions": { "0.99.0": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.99.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/typescript", packument.to_string().into_bytes());
    environment.add_remote_file_bytes(
      "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.99.0.tgz",
      create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]),
    );

    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:@dprint/typescript", &config_path, true, true, &environment)
      .await
      .unwrap();
    assert_eq!(result.url, "npm:@dprint/typescript");
    assert_eq!(result.package_json_addition, Some(("@dprint/typescript".to_string(), "^0.99.0".to_string())),);
  }

  #[tokio::test]
  async fn npm_add_package_json_detects_process_path_from_tarball() {
    // --package-json is usually run before the package is installed, so detect
    // the kind from the registry tarball and write `/plugin.json` even though
    // node_modules has nothing yet. No checksum is written: the unversioned
    // form resolves from node_modules, which doesn't require one.
    use crate::test_helpers::create_test_npm_tarball;
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.0.0" },
      "versions": { "1.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    environment.add_remote_file_bytes(
      "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
      create_test_npm_tarball(&[("package/plugin.json", b"{}")]),
    );

    let config_path = environment.canonicalize("/dprint.json").unwrap();
    let result = call_resolve_npm_plugin_to_add("npm:foo", &config_path, true, true, &environment).await.unwrap();
    assert_eq!(result.url, "npm:foo/plugin.json");
    assert_eq!(result.package_json_addition, Some(("foo".to_string(), "^1.0.0".to_string())));
  }

  #[tokio::test]
  async fn apply_package_json_additions_appends_to_devdependencies() {
    // baseline: an existing package.json without devDependencies grows a
    // new section with the queued entries.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment.write_file("/package.json", "{\n  \"name\": \"app\"\n}\n").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();

    super::apply_package_json_additions(&config_path, &[("@dprint/typescript".to_string(), "^0.99.0".to_string())], &environment).unwrap();

    let pkg = environment.read_file("/package.json").unwrap();
    assert!(pkg.contains("\"devDependencies\""), "got: {pkg}");
    assert!(pkg.contains("\"@dprint/typescript\": \"^0.99.0\""), "got: {pkg}");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn apply_package_json_additions_overwrites_existing_entry() {
    // if the package is already listed (different version), the entry
    // is updated rather than duplicated.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    environment
      .write_file(
        "/package.json",
        "{\n  \"devDependencies\": {\n    \"@dprint/typescript\": \"^0.50.0\"\n  }\n}\n",
      )
      .unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();

    super::apply_package_json_additions(&config_path, &[("@dprint/typescript".to_string(), "^0.99.0".to_string())], &environment).unwrap();

    let pkg = environment.read_file("/package.json").unwrap();
    assert!(pkg.contains("\"@dprint/typescript\": \"^0.99.0\""), "got: {pkg}");
    assert!(!pkg.contains("\"^0.50.0\""), "old version should be replaced, got: {pkg}");
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn apply_package_json_additions_walks_up_to_workspace_root() {
    // monorepo: dprint.json sits in a child workspace, package.json is at
    // the repo root. We should land the entry at the root rather than
    // demand a per-workspace package.json.
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/repo/packages/web").unwrap();
    environment.write_file("/repo/packages/web/dprint.json", "{}").unwrap();
    environment.write_file("/repo/package.json", "{\n  \"name\": \"root\"\n}\n").unwrap();
    let config_path = environment.canonicalize("/repo/packages/web/dprint.json").unwrap();

    super::apply_package_json_additions(&config_path, &[("@dprint/typescript".to_string(), "^0.99.0".to_string())], &environment).unwrap();

    let pkg = environment.read_file("/repo/package.json").unwrap();
    assert!(pkg.contains("\"@dprint/typescript\": \"^0.99.0\""), "got: {pkg}");
    // child workspace package.json wasn't created
    assert!(environment.read_file("/repo/packages/web/package.json").is_err());
    let _ = environment.take_stderr_messages();
  }

  #[tokio::test]
  async fn apply_package_json_additions_warns_when_no_package_json_anywhere() {
    // dprint.json was still updated by the caller before we ran, so
    // bailing here would leave the user with a half-applied add. Warn
    // and continue so they can recover by adding a package.json and
    // re-running.
    let environment = TestEnvironment::new();
    environment.write_file("/dprint.json", "{}").unwrap();
    let config_path = environment.canonicalize("/dprint.json").unwrap();
    super::apply_package_json_additions(&config_path, &[("@dprint/typescript".to_string(), "^0.99.0".to_string())], &environment).unwrap();
    let stderr = environment.take_stderr_messages();
    assert!(
      stderr.iter().any(|m| m.contains("no package.json was found") && m.contains("npm init")),
      "expected warn, got: {stderr:?}"
    );
  }

  #[test]
  fn config_add_npm_replaces_existing_entry_for_same_package() {
    // re-adding a package that's already present should replace the existing
    // entry (any version) rather than append a duplicate. A versioned add
    // without a path inspects the package to detect its kind, so mock the
    // registry with a wasm tarball.
    use crate::test_helpers::create_test_npm_tarball;
    let packument = serde_json::json!({
      "dist-tags": { "latest": "2.0.0" },
      "versions": { "2.0.0": { "dist": { "tarball": "https://registry.npmjs.org/test-plugin/-/test-plugin-2.0.0.tgz" } } }
    });
    let environment = TestEnvironmentBuilder::new()
      .with_local_config("/dprint.json", |c| {
        c.add_plugin("npm:test-plugin@1.0.0");
        c.add_plugin("npm:other@1.0.0");
      })
      .add_remote_file_bytes("https://registry.npmjs.org/test-plugin", packument.to_string().into_bytes())
      .add_remote_file_bytes(
        "https://registry.npmjs.org/test-plugin/-/test-plugin-2.0.0.tgz",
        create_test_npm_tarball(&[("package/plugin.wasm", b"\0asm")]),
      )
      .build();

    run_test_cli(vec!["config", "add", "npm:test-plugin@2.0.0"], &environment).unwrap();

    let dprint_json = environment.read_file("/dprint.json").unwrap();
    assert!(dprint_json.contains("npm:test-plugin@2.0.0"), "new version added, got: {dprint_json}");
    assert!(!dprint_json.contains("npm:test-plugin@1.0.0"), "old entry removed, got: {dprint_json}");
    assert!(dprint_json.contains("npm:other@1.0.0"), "unrelated entry kept, got: {dprint_json}");
    assert_eq!(
      dprint_json.matches("npm:test-plugin").count(),
      1,
      "exactly one entry for the package, got: {dprint_json}"
    );
    let _ = environment.take_stdout_messages();
    let _ = environment.take_stderr_messages();
  }

  #[test]
  fn config_update_skips_unversioned_npm_specifiers() {
    // unversioned npm specifiers track node_modules; their versions are
    // managed by npm/package-lock.json, so `dprint config update` shouldn't
    // try to bump them. Surface that with a warn so the user knows we saw
    // the entry and intentionally skipped it.
    use crate::test_helpers::WASM_PLUGIN_BYTES;

    let environment = TestEnvironmentBuilder::new()
      .with_local_config("/dprint.json", |c| {
        c.add_plugin("npm:test-plugin");
      })
      .write_file("/node_modules/test-plugin/plugin.wasm", WASM_PLUGIN_BYTES)
      .build();

    run_test_cli(vec!["config", "update"], &environment).unwrap();
    let stderr = environment.take_stderr_messages();
    assert!(
      stderr
        .iter()
        .any(|m| m.contains("unversioned npm specifier") && m.contains("update via your package manager")),
      "expected skip warning, got: {stderr:?}"
    );

    // dprint.json should still carry the unversioned form (skipped, not rewritten)
    let dprint_json = environment.read_file("/dprint.json").unwrap();
    assert!(dprint_json.contains("npm:test-plugin"), "got: {dprint_json}");
    // and no version pin snuck in
    assert!(!dprint_json.contains("npm:test-plugin@"), "got: {dprint_json}");
  }

  #[tokio::test]
  async fn is_in_package_json_deps_warns_on_malformed_package_json() {
    // a corrupt package.json shouldn't be silently treated as "plugin
    // not declared" — the user almost certainly wants to know.
    let environment = TestEnvironment::new();
    environment.write_file("/package.json", "{ not valid json").unwrap();
    let found = super::is_in_package_json_deps("@dprint/typescript", std::path::Path::new("/"), &environment);
    assert!(!found);
    let stderr = environment.take_stderr_messages();
    assert!(
      stderr.iter().any(|m| m.contains("/package.json") && m.contains("failed to parse")),
      "expected parse warning, got: {stderr:?}"
    );
  }
}
