use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use flate2::read::GzDecoder;
use tar::Archive;

use deno_npmrc::RegistryConfig;

use crate::environment::Environment;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::get_sha256_checksum;
use crate::utils::verify_sha256_checksum;

/// Resolved npm registry for a package, including the auth header to send
/// with requests (if the configured `.npmrc` provides one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmRegistryResolution {
  pub url: String,
  pub auth_header: Option<String>,
}

/// The result of resolving an npm plugin specifier.
pub struct NpmResolvedPlugin {
  /// The extracted plugin bytes (wasm binary or plugin.json contents).
  pub plugin_bytes: Vec<u8>,
  /// Whether this is a wasm or process plugin.
  pub plugin_kind: PluginKind,
  /// The local path to the plugin file on disk.
  /// Used as the PathSource for setup so process plugin manifests
  /// can resolve relative URLs against the package directory.
  pub local_path: PathSource,
  /// For npm-resolved process plugins, the per-platform tarball whose contents
  /// `setup_process_plugin` will extract into the plugin cache directory.
  /// Carries the full tarball bytes (verified against the plugin.json
  /// checksum) so the executable can be unpacked alongside any sibling
  /// files it depends on — node_modules-style installs, data files, etc.
  pub pre_resolved_tarball: Option<PreResolvedProcessPluginTarball>,
}

/// Per-platform npm tarball that's been fetched and SHA-verified for a
/// process plugin's plugin.json reference. The tarball is unpacked in
/// full into the plugin cache so it can ship things alongside the binary;
/// `executable_sub_path` is the binary's location inside the tarball's
/// top-level wrapper directory (e.g. `foo` for `package/foo`).
pub struct PreResolvedProcessPluginTarball {
  pub name: String,
  pub version: String,
  pub tarball_bytes: Vec<u8>,
  pub executable_sub_path: String,
}

/// Information about the latest published version of an npm-distributed plugin.
pub struct NpmLatestInfo {
  pub version: String,
  /// SHA-256 of the latest tarball. Populated when `want_tarball_sha` was set
  /// on the request, or whenever the plugin is non-wasm (where a checksum is
  /// always required in the dprint.json specifier).
  pub tarball_sha256: Option<String>,
}

/// Inputs to [`fetch_npm_latest_info`].
pub struct FetchNpmLatestInfo<'a> {
  pub specifier: &'a NpmSpecifier,
  /// Directory to start the `.npmrc` walk from when resolving the registry.
  /// `None` falls back to `~/.npmrc` and then the default registry.
  pub start_dir: Option<&'a Path>,
  /// Force computing the tarball checksum even for wasm plugins. Used on
  /// update when the existing specifier carries a checksum, so the upgrade
  /// pins to the new tarball rather than carrying the stale hash. Non-wasm
  /// plugins always compute the checksum regardless.
  pub want_tarball_sha: bool,
}

/// Fetches the latest version of an npm-distributed plugin (and, when needed,
/// the SHA-256 of its tarball). Used by `dprint config update` and `dprint add`.
pub async fn fetch_npm_latest_info(args: FetchNpmLatestInfo<'_>, environment: &impl Environment) -> Result<NpmLatestInfo> {
  let FetchNpmLatestInfo {
    specifier,
    start_dir,
    want_tarball_sha,
  } = args;
  let registry = resolve_registry_for_package(&specifier.name, start_dir, environment);
  let packument_url_str = get_packument_url(&registry.url, &specifier.name);
  let packument_url = url::Url::parse(&packument_url_str).with_context(|| format!("Failed to parse npm packument URL: {}", packument_url_str))?;
  let (_, packument_file) = environment
    .download_file_with_auth_err_404(&packument_url, registry.auth_header.as_deref())
    .await
    .with_context(|| format!("Failed to fetch npm packument for {}", specifier.name))?;
  let packument: serde_json::Value =
    serde_json::from_slice(&packument_file.content).with_context(|| format!("Failed to parse npm packument for {}", specifier.name))?;

  let latest_version = packument
    .get("dist-tags")
    .and_then(|d| d.get("latest"))
    .and_then(|v| v.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing dist-tags.latest for {}", specifier.name))?
    .to_string();

  let need_tarball_sha = want_tarball_sha || specifier.plugin_kind() != PluginKind::Wasm;
  let tarball_sha256 = if need_tarball_sha {
    let tarball_url_str = get_tarball_url_from_packument(&packument, &latest_version, &specifier.name)?;
    let tarball_url = url::Url::parse(&tarball_url_str).with_context(|| format!("Failed to parse npm tarball URL: {}", tarball_url_str))?;
    let tarball_auth = same_origin_auth(&packument_url, &tarball_url, registry.auth_header.as_deref());
    let (_, tarball_file) = environment
      .download_file_with_auth_err_404(&tarball_url, tarball_auth)
      .await
      .with_context(|| format!("Failed to download npm tarball for {}@{}", specifier.name, latest_version))?;
    Some(get_sha256_checksum(&tarball_file.content))
  } else {
    None
  };

  Ok(NpmLatestInfo {
    version: latest_version,
    tarball_sha256,
  })
}

/// Resolves an npm plugin from the registry (versioned specifier).
/// Downloads the tarball, extracts it to the cache directory, and reads the plugin file.
/// `config_dir` is the dprint config's directory; it seeds the `.npmrc` walk
/// when a process plugin's plugin.json references a per-platform npm package.
pub async fn resolve_npm_from_registry(
  specifier: &NpmSpecifier,
  checksum: Option<&str>,
  registry: &NpmRegistryResolution,
  config_dir: Option<&Path>,
  environment: &impl Environment,
) -> Result<NpmResolvedPlugin> {
  let version = specifier
    .version
    .as_deref()
    .ok_or_else(|| anyhow::anyhow!("Cannot resolve npm plugin without a version from the registry"))?;
  let registry_segment = registry_dir_segment(&registry.url);

  // fetch the packument to get the tarball URL
  let packument_url_str = get_packument_url(&registry.url, &specifier.name);
  let packument_url = url::Url::parse(&packument_url_str).with_context(|| format!("Failed to parse npm packument URL: {}", packument_url_str))?;
  log_debug!(environment, "Fetching npm packument: {}", packument_url);
  let (_, packument_file) = environment
    .download_file_with_auth_err_404(&packument_url, registry.auth_header.as_deref())
    .await
    .with_context(|| format!("Failed to fetch npm packument for {}", specifier.name))?;
  let packument: serde_json::Value =
    serde_json::from_slice(&packument_file.content).with_context(|| format!("Failed to parse npm packument for {}", specifier.name))?;

  let tarball_url_str = get_tarball_url_from_packument(&packument, version, &specifier.name)?;
  let tarball_url = url::Url::parse(&tarball_url_str).with_context(|| format!("Failed to parse npm tarball URL: {}", tarball_url_str))?;
  log_debug!(environment, "Downloading npm tarball: {}", tarball_url);

  // download the tarball — only send the registry auth if the tarball is on
  // the same origin as the registry (don't leak credentials to a CDN)
  let tarball_auth = same_origin_auth(&packument_url, &tarball_url, registry.auth_header.as_deref());
  let (_, tarball_file) = environment
    .download_file_with_auth_err_404(&tarball_url, tarball_auth)
    .await
    .with_context(|| format!("Failed to download npm tarball for {}@{}", specifier.name, version))?;
  let tarball_bytes = tarball_file.content;

  // verify checksum before doing any extraction work
  let plugin_kind = specifier.plugin_kind();
  if let Some(checksum) = checksum {
    if let Err(err) = verify_sha256_checksum(&tarball_bytes, checksum) {
      bail!(
        "Invalid checksum for npm package {}. Check the plugin's release notes for the expected checksum.\n\n{:#}",
        specifier.display(),
        err
      );
    }
  } else if plugin_kind != PluginKind::Wasm {
    bail!(
      concat!(
        "The npm plugin must have a checksum specified for security reasons ",
        "since it is not a Wasm plugin. Check the plugin's release notes for what ",
        "the checksum is or if you trust the source, you may specify: {}@{}"
      ),
      specifier.display(),
      get_sha256_checksum(&tarball_bytes),
    );
  }

  // extract and read the plugin file in a blocking task since
  // tarball decompression and file I/O can be slow
  let extract_dir = get_npm_extract_dir(&registry_segment, &specifier.name, version, environment);
  let plugin_path = specifier.path.clone();
  let specifier_clone = specifier.clone();
  let environment_clone = environment.clone();
  let (plugin_bytes, local_path) = dprint_core::async_runtime::spawn_blocking(move || -> Result<_> {
    let environment = environment_clone;
    extract_tarball_to_dir(&tarball_bytes, &extract_dir, &environment)?;

    let plugin_file_path = extract_dir.join(&plugin_path);
    if !environment.path_exists(&plugin_file_path) {
      bail!(missing_plugin_file_message(
        &specifier_clone,
        &format!("npm package {}", specifier_clone.name),
        &extract_dir,
        &environment,
      ));
    }
    let plugin_bytes = environment
      .read_file_bytes(&plugin_file_path)
      .with_context(|| format!("Failed to read {}", plugin_file_path.display()))?;
    let canonical = environment.canonicalize(&plugin_file_path)?;
    Ok((plugin_bytes, PathSource::new_local(canonical)))
  })
  .await??;

  // process plugins shipped via the npm registry mustn't silently fetch their
  // platform binary over http(s) at format time. For npm references in
  // plugin.json, we fetch the per-platform tarball from the registry and
  // verify its checksum (same flow as the node_modules path). File/relative
  // references resolve against the extract dir via the standard setup flow.
  let pre_resolved_tarball = if plugin_kind == PluginKind::Process {
    try_resolve_process_plugin_per_platform_tarball(&plugin_bytes, config_dir, environment).await?
  } else {
    None
  };

  Ok(NpmResolvedPlugin {
    plugin_bytes,
    plugin_kind,
    local_path,
    pre_resolved_tarball,
  })
}

/// Resolves an npm plugin from node_modules (unversioned specifier). For
/// process plugins, the per-platform binary referenced by plugin.json is
/// fetched from the npm registry (not node_modules), since the checksum
/// inside plugin.json covers the per-platform tarball — see
/// `try_resolve_process_plugin_per_platform_binary`.
pub async fn resolve_npm_from_node_modules(specifier: &NpmSpecifier, config_dir: &Path, environment: &impl Environment) -> Result<NpmResolvedPlugin> {
  let package_dir = match find_package_in_node_modules(&specifier.name, config_dir, environment) {
    Some(dir) => dir,
    None => bail!(node_modules_missing_message(specifier, Some(config_dir), environment).await),
  };
  let plugin_path = package_dir.join(&specifier.path);
  if !environment.path_exists(&plugin_path) {
    bail!(missing_plugin_file_message(
      specifier,
      &package_dir.display().to_string(),
      &package_dir,
      environment,
    ));
  }
  let canonical = environment.canonicalize(&plugin_path)?;
  let local_path = PathSource::new_local(canonical.clone());
  let plugin_bytes = environment
    .read_file_bytes(canonical.as_ref())
    .with_context(|| format!("Failed to read {}", canonical.display()))?;

  let pre_resolved_tarball = if specifier.plugin_kind() == PluginKind::Process {
    try_resolve_process_plugin_per_platform_tarball(&plugin_bytes, Some(config_dir), environment).await?
  } else {
    None
  };

  Ok(NpmResolvedPlugin {
    plugin_bytes,
    plugin_kind: specifier.plugin_kind(),
    local_path,
    pre_resolved_tarball,
  })
}

/// Locates the canonical local path an unversioned npm specifier resolves to,
/// without reading the plugin file or doing process-plugin dep resolution.
/// Sync, so the error here is the bare "not installed" hint — the async
/// resolve path enriches the message with a versioned suggestion via
/// [`node_modules_missing_message`].
pub fn find_npm_plugin_local_path(specifier: &NpmSpecifier, config_dir: &Path, environment: &impl Environment) -> Result<PathSource> {
  let package_dir = find_package_in_node_modules(&specifier.name, config_dir, environment).ok_or_else(|| {
    anyhow::anyhow!(
      "Could not find {} in node_modules. Make sure the package is installed (npm install {}).",
      specifier.name,
      specifier.name,
    )
  })?;
  let plugin_path = package_dir.join(&specifier.path);

  if !environment.path_exists(&plugin_path) {
    bail!(missing_plugin_file_message(
      specifier,
      &package_dir.display().to_string(),
      &package_dir,
      environment,
    ));
  }

  let canonical = environment.canonicalize(&plugin_path)?;
  Ok(PathSource::new_local(canonical))
}

/// Builds the error message for a missing plugin file inside an npm package.
/// When the requested file is `plugin.wasm` (or vice-versa) and the other
/// recognized plugin file is actually present, suggest the corrected
/// specifier so users hit the helpful error instead of "Is the package a
/// dprint plugin?".
fn missing_plugin_file_message(specifier: &NpmSpecifier, package_display: &str, package_dir: &Path, environment: &impl Environment) -> String {
  if let Some(alternate) = alternate_plugin_filename(&specifier.path, package_dir, environment) {
    let suggestion = npm_specifier_with_path(specifier, alternate);
    return format!(
      "Could not find {} in {}. The package contains {} instead — reference it as `{}`.",
      specifier.path, package_display, alternate, suggestion,
    );
  }
  format!("Could not find {} in {}. Is the package a dprint plugin?", specifier.path, package_display)
}

/// If the requested path is one recognized plugin filename and the *other*
/// recognized plugin filename actually exists in the package directory,
/// return that other filename.
fn alternate_plugin_filename(requested: &str, package_dir: &Path, environment: &impl Environment) -> Option<&'static str> {
  let candidate = if requested.eq_ignore_ascii_case("plugin.wasm") {
    "plugin.json"
  } else if requested.eq_ignore_ascii_case("plugin.json") {
    "plugin.wasm"
  } else {
    return None;
  };
  if environment.path_exists(package_dir.join(candidate)) {
    Some(candidate)
  } else {
    None
  }
}

fn npm_specifier_with_path(specifier: &NpmSpecifier, path: &str) -> String {
  match &specifier.version {
    Some(version) => format!("npm:{}@{}/{}", specifier.name, version, path),
    None => format!("npm:{}/{}", specifier.name, path),
  }
}

/// Reads a process plugin manifest (plugin.json) and, if the platform-specific
/// reference is an `npm:` specifier, fetches the per-platform package's
/// tarball from the npm registry and verifies its SHA-256 against the
/// plugin.json checksum. The full tarball bytes are handed back to
/// `setup_process_plugin`, which unpacks them into the plugin cache
/// directory — extracting the whole package (not just the named binary)
/// so the executable can sit alongside any DLLs / data files it ships.
///
/// Returns `None` for non-npm references so the caller falls back to the
/// standard flow (relative paths / `file:///` resolved against plugin.json's
/// directory). `http(s)://` is rejected so an npm-installed plugin can't
/// silently fetch from the network.
async fn try_resolve_process_plugin_per_platform_tarball(
  plugin_json_bytes: &[u8],
  config_dir: Option<&Path>,
  environment: &impl Environment,
) -> Result<Option<PreResolvedProcessPluginTarball>> {
  use crate::plugins::implementations::get_process_plugin_os_path;
  use crate::plugins::implementations::parse_process_plugin_file;

  let plugin_file = parse_process_plugin_file(plugin_json_bytes).context("Failed to parse process plugin manifest (plugin.json)")?;
  let os_path = get_process_plugin_os_path(&plugin_file, environment)?;

  if !os_path.reference.starts_with("npm:") {
    bail_if_http_reference(&plugin_file.name, &os_path.reference)?;
    return Ok(None);
  }

  let parsed = crate::utils::parse_npm_specifier(&os_path.reference)?;
  let version = parsed
    .specifier
    .version
    .as_deref()
    .ok_or_else(|| anyhow::anyhow!("npm reference in plugin '{}' must include a version: {}", plugin_file.name, os_path.reference,))?;

  let registry = resolve_registry_for_package(&parsed.specifier.name, config_dir, environment);
  let tarball_bytes = fetch_and_verify_npm_tarball(&parsed.specifier.name, version, &os_path.checksum, &registry, environment)
    .await
    .with_context(|| format!("Resolving npm dependency for process plugin '{}'", plugin_file.name))?;

  Ok(Some(PreResolvedProcessPluginTarball {
    name: plugin_file.name,
    version: plugin_file.version,
    tarball_bytes,
    executable_sub_path: parsed.specifier.path,
  }))
}

/// Fetches `name@version` from `registry` and verifies its SHA-256 against
/// `expected_checksum`. Returns the tarball bytes — the caller decides
/// where (if anywhere) to extract them. Always re-fetches and re-verifies
/// on every call so a registry that silently swaps the tarball's contents
/// is detected immediately.
async fn fetch_and_verify_npm_tarball(
  name: &str,
  version: &str,
  expected_checksum: &str,
  registry: &NpmRegistryResolution,
  environment: &impl Environment,
) -> Result<Vec<u8>> {
  let packument_url_str = get_packument_url(&registry.url, name);
  let packument_url = url::Url::parse(&packument_url_str).with_context(|| format!("Failed to parse npm packument URL: {}", packument_url_str))?;
  let (_, packument_file) = environment
    .download_file_with_auth_err_404(&packument_url, registry.auth_header.as_deref())
    .await
    .with_context(|| format!("Failed to fetch npm packument for {}", name))?;
  let packument: serde_json::Value = serde_json::from_slice(&packument_file.content).with_context(|| format!("Failed to parse npm packument for {}", name))?;

  let tarball_url_str = get_tarball_url_from_packument(&packument, version, name)?;
  let tarball_url = url::Url::parse(&tarball_url_str).with_context(|| format!("Failed to parse npm tarball URL: {}", tarball_url_str))?;
  let tarball_auth = same_origin_auth(&packument_url, &tarball_url, registry.auth_header.as_deref());
  let (_, tarball_file) = environment
    .download_file_with_auth_err_404(&tarball_url, tarball_auth)
    .await
    .with_context(|| format!("Failed to download npm tarball for {}@{}", name, version))?;
  let tarball_bytes = tarball_file.content;

  if let Err(err) = verify_sha256_checksum(&tarball_bytes, expected_checksum) {
    bail!(
      "Invalid checksum for npm package {}@{}. The tarball's contents don't match the expected SHA-256.\n\n{:#}",
      name,
      version,
      err,
    );
  }

  Ok(tarball_bytes)
}

fn bail_if_http_reference(plugin_name: &str, reference: &str) -> Result<()> {
  let scheme = url::Url::parse(reference).ok().map(|u| u.scheme().to_string());
  if matches!(scheme.as_deref(), Some("http" | "https")) {
    bail!(
      concat!(
        "Process plugin '{}' was installed via npm but its plugin.json references the platform ",
        "binary over the network ({}). Network references aren't allowed for npm-installed plugins; ",
        "the plugin author needs to ship the binary inside the npm package or as a separate npm package.",
      ),
      plugin_name,
      reference,
    );
  }
  Ok(())
}

/// Finds a .zip file in the given package directory.
/// Returns the directory where an npm package tarball should be extracted.
/// Namespaced by the registry so the same name@version from different registries
/// (e.g. public npmjs.org vs a private registry) do not collide.
pub(super) fn get_npm_extract_dir(registry_segment: &str, package_name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  // use a sanitized name for the directory (replace / with __)
  let dir_name = format!("{}@{}", package_name.replace('/', "__"), version);
  environment.get_cache_dir().join("npm").join(registry_segment).join(dir_name)
}

/// Returns a filesystem- and key-safe segment identifying a registry by host
/// (and port, if non-default). For URLs we can't parse or that have no host,
/// falls back to `unknown_<hash>` so distinct unparseable URLs land in
/// different cache directories instead of colliding under a shared `unknown`.
pub(super) fn registry_dir_segment(registry_url: &str) -> String {
  let fallback = || format!("unknown_{:016x}", crate::utils::get_bytes_hash(registry_url.as_bytes()));
  let Ok(url) = url::Url::parse(registry_url) else {
    return fallback();
  };
  let Some(host) = url.host_str() else {
    return fallback();
  };
  match url.port() {
    Some(port) => format!("{host}_{port}"),
    None => host.to_string(),
  }
}

/// Extracts an npm tarball to a directory on disk.
///
/// Idempotent and concurrency-safe:
/// - If `dest_dir` already exists, returns immediately. `name@version` is
///   immutable on npm, and `dest_dir` only appears after a complete extract
///   (we populate a temp dir first, then rename), so existence implies a
///   prior successful extract.
/// - Each call uses a uniquely-named temp dir, so two concurrent extracts
///   (different processes, or otherwise outside the in-process fs lock pool)
///   never trample each other's intermediate state.
/// - If the final rename fails because a racing extract finished first, we
///   discard our copy and use the winner's `dest_dir`.
///
/// Strips the first path component (usually `package/`) from each entry.
fn extract_tarball_to_dir(tarball_bytes: &[u8], dest_dir: &Path, environment: &impl Environment) -> Result<()> {
  use crate::utils::fs::get_atomic_path;

  if environment.path_exists(dest_dir) {
    return Ok(());
  }

  let temp_dir = get_atomic_path(environment, dest_dir);
  environment.mk_dir_all(&temp_dir)?;

  if let Err(err) = extract_tarball_to_dir_inner(tarball_bytes, &temp_dir, environment) {
    let _ = environment.remove_dir_all(&temp_dir);
    return Err(err);
  }

  match environment.rename(&temp_dir, dest_dir) {
    Ok(()) => Ok(()),
    Err(err) => {
      if environment.path_exists(dest_dir) {
        // another concurrent extract finished first — discard our copy and
        // use theirs (immutable name@version means contents are equivalent)
        let _ = environment.remove_dir_all(&temp_dir);
        Ok(())
      } else {
        let _ = environment.remove_dir_all(&temp_dir);
        Err(err.into())
      }
    }
  }
}

/// Extracts an npm tarball into `dest_dir`, replacing any existing contents.
/// Same wrapper-stripping / path-traversal / permission-preserving rules as
/// [`extract_tarball_to_dir`], but for caches whose contents are *not*
/// content-addressable (e.g. the per-plugin cache, which gets rewritten
/// whenever the source plugin.json changes). The extract goes through a
/// sibling temp dir so a crash mid-extract can't leave the destination
/// half-populated; the caller is responsible for serializing extracts
/// against the same `dest_dir` via fs locks.
pub(in crate::plugins) fn extract_tarball_replacing(tarball_bytes: &[u8], dest_dir: &Path, environment: &impl Environment) -> Result<()> {
  use crate::utils::fs::get_atomic_path;

  let temp_dir = get_atomic_path(environment, dest_dir);
  environment.mk_dir_all(&temp_dir)?;

  if let Err(err) = extract_tarball_to_dir_inner(tarball_bytes, &temp_dir, environment) {
    let _ = environment.remove_dir_all(&temp_dir);
    return Err(err);
  }

  let _ = environment.remove_dir_all(dest_dir);
  if let Err(err) = environment.rename(&temp_dir, dest_dir) {
    let _ = environment.remove_dir_all(&temp_dir);
    return Err(err.into());
  }
  Ok(())
}

fn extract_tarball_to_dir_inner(tarball_bytes: &[u8], output_dir: &Path, environment: &impl Environment) -> Result<()> {
  let decoder = GzDecoder::new(tarball_bytes);
  let mut archive = Archive::new(decoder);
  // npm tarballs wrap every entry under a single top-level directory (usually
  // "package/"). Lock in that wrapper from the first entry and reject any
  // entry that doesn't share it, otherwise files outside the wrapper would
  // be silently dropped.
  let mut wrapper: Option<std::ffi::OsString> = None;
  let mut files_written: usize = 0;

  for entry in archive.entries().context("Failed to read npm tarball entries")? {
    let mut entry = entry.context("Failed to read npm tarball entry")?;
    let entry_type = entry.header().entry_type();

    // skip symlinks, hardlinks, and other special entries
    match entry_type {
      tar::EntryType::Regular | tar::EntryType::Directory => {}
      tar::EntryType::Symlink | tar::EntryType::Link => {
        // npm doesn't support symlinks/hardlinks in packages
        continue;
      }
      tar::EntryType::XGlobalHeader => continue,
      _ => continue,
    }

    let path = entry.path().context("Failed to get entry path")?.to_path_buf();

    // GNU tar prefixes entries with `./` by default; skip those leading
    // `CurDir`s so the wrapper-detection sees the real first directory.
    let mut components = path.components().peekable();
    while let Some(std::path::Component::CurDir) = components.peek() {
      components.next();
    }
    let Some(first) = components.next() else {
      continue;
    };
    // require a normal directory component as the wrapper. Absolute paths
    // (RootDir / Prefix) and `..` (ParentDir) at the top of an entry would
    // otherwise pin `wrapper` to a value we'd silently apply to subsequent
    // entries, mis-shaping the extract.
    let std::path::Component::Normal(first_name) = first else {
      bail!(
        "Refusing to extract npm tarball entry with non-relative top-level component: {}",
        path.display(),
      );
    };
    let first_os = first_name.to_os_string();
    match &wrapper {
      None => wrapper = Some(first_os),
      Some(existing) if existing == &first_os => {}
      Some(existing) => {
        bail!(
          "Inconsistent npm tarball: expected all entries under '{}/' but found '{}'",
          existing.to_string_lossy(),
          path.display(),
        );
      }
    }

    let relative: PathBuf = components.collect();
    if relative.as_os_str().is_empty() {
      continue;
    }

    let dest_path = output_dir.join(&relative);

    // path traversal check: ensure the resolved path stays within the output directory
    let normalized = normalize_path(&dest_path);
    if !normalized.starts_with(output_dir) {
      bail!("Refusing to extract tarball entry outside output directory: {}", path.display());
    }

    if entry_type == tar::EntryType::Directory {
      environment.mk_dir_all(&dest_path)?;
      continue;
    }

    // regular file
    if let Some(parent) = dest_path.parent() {
      environment.mk_dir_all(parent)?;
    }

    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    environment.write_file_bytes(&dest_path, &bytes)?;
    files_written += 1;

    // preserve executable permissions on unix
    #[cfg(unix)]
    if let Ok(mode) = entry.header().mode()
      && mode != 0o644
    {
      use sys_traits::FsSetPermissions;
      environment
        .fs_set_permissions(&dest_path, mode)
        .with_context(|| format!("Failed to set permissions on {}", dest_path.display()))?;
    }
  }

  if files_written == 0 {
    bail!("npm tarball contained no extractable files (expected at least one file under a wrapper directory)");
  }

  Ok(())
}

/// Normalizes a path by resolving `.` and `..` components without accessing the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
  let mut result = PathBuf::new();
  for component in path.components() {
    match component {
      std::path::Component::ParentDir => {
        result.pop();
      }
      std::path::Component::CurDir => {}
      other => result.push(other),
    }
  }
  result
}

/// Resolves the npm registry URL and credentials for a package, checking (in order):
/// 1. NPM_CONFIG_REGISTRY env var (no credentials)
/// 2. .npmrc files walking up from `start_dir` — keep walking past `.npmrc`s
///    that don't apply to this package's scope
/// 3. ~/.npmrc
/// 4. https://registry.npmjs.org (no credentials)
pub fn resolve_registry_for_package(package_name: &str, start_dir: Option<&Path>, environment: &impl Environment) -> NpmRegistryResolution {
  // env vars take precedence over .npmrc — but they only set the URL,
  // never auth, so we can return immediately.
  if let Some(registry) = environment.env_var("NPM_CONFIG_REGISTRY") {
    let registry = registry.to_string_lossy().to_string();
    return NpmRegistryResolution {
      url: registry.trim_end_matches('/').to_string(),
      auth_header: None,
    };
  }

  // walk up from the config file's directory checking for .npmrc files
  if let Some(start) = start_dir {
    for dir in start.ancestors() {
      if let Some(info) = resolve_registry_from_npmrc(package_name, &dir.join(".npmrc"), environment) {
        return info;
      }
    }
  }

  // user-level ~/.npmrc
  if let Some(home_dir) = environment.get_home_dir()
    && let Some(info) = resolve_registry_from_npmrc(package_name, &home_dir.join(".npmrc"), environment)
  {
    return info;
  }

  NpmRegistryResolution {
    url: deno_npmrc::NPM_DEFAULT_REGISTRY.to_string(),
    auth_header: None,
  }
}

/// Parses a single .npmrc file and resolves the registry for a package.
/// Returns `None` if the file doesn't exist, or if it doesn't configure a registry
/// that applies to this package (so the caller keeps walking).
fn resolve_registry_from_npmrc(package_name: &str, npmrc_path: &Path, environment: &impl Environment) -> Option<NpmRegistryResolution> {
  let text = environment.read_file(npmrc_path).ok()?;
  let npmrc = deno_npmrc::NpmRc::parse(environment, &text).ok()?;

  // figure out whether this .npmrc actually applies to this package — either
  // a scope registry matching the package's scope, or a default registry.
  let scope = scope_of(package_name);
  let has_default = npmrc.registry.is_some();
  let has_scope = scope.is_some_and(|s| npmrc.scope_registries.contains_key(s));
  if !has_default && !has_scope {
    return None;
  }

  let default_url = url::Url::parse(deno_npmrc::NPM_DEFAULT_REGISTRY).unwrap();
  let registry_url = deno_npmrc::NpmRegistryUrl {
    url: default_url,
    from_env: false,
  };
  let resolved = npmrc.as_resolved(&registry_url).ok()?;
  let url = resolved.get_registry_url(package_name).as_str().trim_end_matches('/').to_string();
  let auth_header = compute_auth_header(resolved.get_registry_config(package_name).as_ref(), environment);

  Some(NpmRegistryResolution { url, auth_header })
}

/// Returns the scope (without the `@`) for a scoped package, or `None` for
/// unscoped packages.
fn scope_of(package_name: &str) -> Option<&str> {
  package_name.strip_prefix('@')?.split_once('/').map(|(scope, _)| scope)
}

/// Builds an HTTP `Authorization` header value from a registry's `.npmrc`
/// credentials. Supports `_authToken` (Bearer), `_auth` (Basic), and the
/// `username` + `_password` pair (Basic, base64-encoded here — npm stores
/// `_password` itself as base64 over the cleartext password). Warns when
/// credentials are configured but unusable rather than silently sending
/// an unauthenticated request, since the resulting 401 from the registry
/// is otherwise hard to trace back to a misconfigured `.npmrc`.
fn compute_auth_header(config: &RegistryConfig, environment: &impl Environment) -> Option<String> {
  use base64::Engine;
  if let Some(token) = &config.auth_token {
    return Some(format!("Bearer {}", token));
  }
  if let Some(auth) = &config.auth {
    return Some(format!("Basic {}", auth));
  }
  if let (Some(username), Some(password_b64)) = (&config.username, &config.password) {
    // npm stores `_password` as base64 of the cleartext password; we need to
    // send `Basic base64(username:password)`. Decode then re-encode.
    let password_bytes = match base64::engine::general_purpose::STANDARD.decode(password_b64.as_bytes()) {
      Ok(bytes) => bytes,
      Err(err) => {
        log_warn!(
          environment,
          "Ignoring .npmrc _password for user '{}': not valid base64 ({}). Request will be sent unauthenticated.",
          username,
          err,
        );
        return None;
      }
    };
    let password = match String::from_utf8(password_bytes) {
      Ok(s) => s,
      Err(_) => {
        log_warn!(
          environment,
          "Ignoring .npmrc _password for user '{}': decoded bytes are not valid UTF-8. Request will be sent unauthenticated.",
          username,
        );
        return None;
      }
    };
    let credentials = format!("{}:{}", username, password);
    return Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes())));
  }
  // username without _password (or vice versa) is a partial config; warn so
  // the user knows we saw it and chose not to use it.
  if config.username.is_some() || config.password.is_some() {
    log_warn!(
      environment,
      "Ignoring .npmrc credentials: 'username' and '_password' must both be set (saw only one). Request will be sent unauthenticated.",
    );
  }
  None
}

/// Returns `auth` only when `other` shares an origin (scheme + host + port)
/// with `registry`. Tarballs/packuments sometimes live on a CDN; the registry
/// credentials must not be sent there.
fn same_origin_auth<'a>(registry: &url::Url, other: &url::Url, auth: Option<&'a str>) -> Option<&'a str> {
  let same =
    registry.scheme() == other.scheme() && registry.host_str() == other.host_str() && registry.port_or_known_default() == other.port_or_known_default();
  if same { auth } else { None }
}

fn get_packument_url(registry_url: &str, package_name: &str) -> String {
  format!("{}/{}", registry_url, package_name)
}

fn get_tarball_url_from_packument(packument: &serde_json::Value, version: &str, package_name: &str) -> Result<String> {
  let versions = packument
    .get("versions")
    .and_then(|v| v.as_object())
    .ok_or_else(|| anyhow::anyhow!("Invalid packument for {}: missing 'versions' object", package_name))?;

  let version_obj = versions
    .get(version)
    .and_then(|v| v.as_object())
    .ok_or_else(|| anyhow::anyhow!("Version {} not found for package {}", version, package_name))?;

  let tarball_url = version_obj
    .get("dist")
    .and_then(|d| d.get("tarball"))
    .and_then(|t| t.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing tarball URL for {}@{}", package_name, version))?;

  Ok(tarball_url.to_string())
}

/// Walks up from `start_dir` looking for `node_modules/{package_name}/`.
/// Returns `None` if not installed anywhere along the ancestor chain.
fn find_package_in_node_modules(package_name: &str, start_dir: &Path, environment: &impl Environment) -> Option<std::path::PathBuf> {
  for dir in start_dir.ancestors() {
    let candidate = dir.join("node_modules").join(package_name);
    if environment.path_exists(&candidate) {
      return Some(candidate);
    }
  }
  None
}

/// Builds the user-facing "package not in node_modules" error. When the
/// caller has async context (the user-facing resolve path), it also
/// suggests a concrete `npm:<name>@<version>` specifier built from the npm
/// registry's `dist-tags.latest`; when that lookup fails we fall back to
/// the bare `npm install` hint. We deliberately don't append the tarball
/// checksum here — for non-wasm plugins the next resolve step bails with a
/// "must have a checksum" error that quotes the exact value to use.
async fn node_modules_missing_message(specifier: &NpmSpecifier, start_dir: Option<&Path>, environment: &impl Environment) -> String {
  match fetch_npm_latest_version(&specifier.name, start_dir, environment).await {
    Some(version) => {
      // hide the default `plugin.wasm` path so the suggestion matches what
      // a user would actually type. Non-default paths (e.g. `plugin.json`)
      // are kept verbatim.
      let suggestion = if specifier.path == "plugin.wasm" {
        format!("npm:{}@{}", specifier.name, version)
      } else {
        format!("npm:{}@{}/{}", specifier.name, version, specifier.path)
      };
      format!(
        concat!(
          "Could not find {} in node_modules.\n",
          "\n",
          "1. Make sure the package is installed (ex. npm install {})\n",
          "2. OR specify a version (ex. {})",
        ),
        specifier.name, specifier.name, suggestion,
      )
    }
    None => format!(
      "Could not find {} in node_modules. Make sure the package is installed (npm install {}).",
      specifier.name, specifier.name,
    ),
  }
}

/// Reads `dist-tags.latest` from the registry's packument for a single
/// package. Returns `None` on any network/parse failure — callers use this
/// as a best-effort hint, not a hard requirement, so we don't bubble the
/// error up. Unlike [`fetch_npm_latest_info`] this never downloads the
/// tarball, so it's cheap enough to call inside an error path.
async fn fetch_npm_latest_version(package_name: &str, start_dir: Option<&Path>, environment: &impl Environment) -> Option<String> {
  let registry = resolve_registry_for_package(package_name, start_dir, environment);
  let packument_url = url::Url::parse(&get_packument_url(&registry.url, package_name)).ok()?;
  let (_, packument_file) = environment
    .download_file_with_auth_err_404(&packument_url, registry.auth_header.as_deref())
    .await
    .ok()?;
  let packument: serde_json::Value = serde_json::from_slice(&packument_file.content).ok()?;
  packument
    .get("dist-tags")
    .and_then(|d| d.get("latest"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn compute_auth_header_supports_auth_token_and_auth() {
    use crate::environment::TestEnvironment;
    let env = TestEnvironment::new();
    let mut cfg = RegistryConfig::default();
    assert_eq!(compute_auth_header(&cfg, &env), None);

    cfg.auth_token = Some("tok".to_string());
    assert_eq!(compute_auth_header(&cfg, &env), Some("Bearer tok".to_string()));

    cfg.auth_token = None;
    cfg.auth = Some("dXNlcjpwd2Q=".to_string());
    assert_eq!(compute_auth_header(&cfg, &env), Some("Basic dXNlcjpwd2Q=".to_string()));

    // auth_token wins over auth when both are present
    cfg.auth_token = Some("tok".to_string());
    assert_eq!(compute_auth_header(&cfg, &env), Some("Bearer tok".to_string()));
  }

  #[test]
  fn compute_auth_header_supports_username_and_password() {
    use crate::environment::TestEnvironment;
    use base64::Engine;
    let env = TestEnvironment::new();
    // npm stores _password as base64 of the cleartext password
    let password_b64 = base64::engine::general_purpose::STANDARD.encode(b"pwd");
    let cfg = RegistryConfig {
      username: Some("user".to_string()),
      password: Some(password_b64),
      ..Default::default()
    };
    // header is base64(user:pwd) which matches the well-known _auth example
    assert_eq!(compute_auth_header(&cfg, &env), Some("Basic dXNlcjpwd2Q=".to_string()));
  }

  #[test]
  fn compute_auth_header_username_only_warns_and_returns_none() {
    use crate::environment::TestEnvironment;
    let env = TestEnvironment::new();
    let cfg = RegistryConfig {
      username: Some("user".to_string()),
      ..Default::default()
    };
    assert_eq!(compute_auth_header(&cfg, &env), None);
    let stderr = env.take_stderr_messages();
    assert!(
      stderr.iter().any(|m| m.contains("'username' and '_password' must both be set")),
      "expected partial-config warning, got: {stderr:?}"
    );
  }

  #[test]
  fn compute_auth_header_invalid_base64_password_warns_and_returns_none() {
    // a misconfigured _password (not valid base64) shouldn't silently produce
    // an unauthenticated request; the user gets a 401 with no clue why.
    use crate::environment::TestEnvironment;
    let env = TestEnvironment::new();
    let cfg = RegistryConfig {
      username: Some("user".to_string()),
      password: Some("!!!not base64!!!".to_string()),
      ..Default::default()
    };
    assert_eq!(compute_auth_header(&cfg, &env), None);
    let stderr = env.take_stderr_messages();
    assert!(
      stderr.iter().any(|m| m.contains("not valid base64") && m.contains("user")),
      "expected base64 warning, got: {stderr:?}"
    );
  }

  #[test]
  fn compute_auth_header_non_utf8_password_warns_and_returns_none() {
    use crate::environment::TestEnvironment;
    use base64::Engine;
    let env = TestEnvironment::new();
    // base64 of a non-UTF-8 byte sequence
    let password_b64 = base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe, 0xfd]);
    let cfg = RegistryConfig {
      username: Some("user".to_string()),
      password: Some(password_b64),
      ..Default::default()
    };
    assert_eq!(compute_auth_header(&cfg, &env), None);
    let stderr = env.take_stderr_messages();
    assert!(stderr.iter().any(|m| m.contains("not valid UTF-8")), "expected utf-8 warning, got: {stderr:?}");
  }

  #[tokio::test]
  async fn resolve_registry_walks_past_unrelated_npmrc() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    // child .npmrc configures a scope that is not our package's scope —
    // the walk should not stop here. Parent .npmrc configures our scope.
    environment.mk_dir_all("/repo").unwrap();
    environment.write_file("/repo/.npmrc", "@other:registry=https://other.example.com").unwrap();
    environment.write_file("/.npmrc", "@dprint:registry=https://dprint.example.com").unwrap();
    let info = resolve_registry_for_package("@dprint/typescript", Some(std::path::Path::new("/repo")), &environment);
    assert_eq!(info.url, "https://dprint.example.com");
  }

  #[tokio::test]
  async fn resolve_registry_picks_up_auth_token() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/repo").unwrap();
    environment
      .write_file(
        "/repo/.npmrc",
        "@dprint:registry=https://dprint.example.com\n//dprint.example.com/:_authToken=MYTOKEN",
      )
      .unwrap();
    let info = resolve_registry_for_package("@dprint/typescript", Some(std::path::Path::new("/repo")), &environment);
    assert_eq!(info.url, "https://dprint.example.com");
    assert_eq!(info.auth_header.as_deref(), Some("Bearer MYTOKEN"));
  }

  #[tokio::test]
  async fn fetch_npm_latest_info_sends_auth_header() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/repo").unwrap();
    environment
      .write_file(
        "/repo/.npmrc",
        "@dprint:registry=https://dprint.example.com\n//dprint.example.com/:_authToken=MYTOKEN",
      )
      .unwrap();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://dprint.example.com/@dprint/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://dprint.example.com/@dprint/foo", packument.to_string().into_bytes());

    let specifier = NpmSpecifier {
      name: "@dprint/foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let info = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &specifier,
        start_dir: Some(std::path::Path::new("/repo")),
        want_tarball_sha: false,
      },
      &environment,
    )
    .await
    .unwrap();
    assert_eq!(info.version, "1.2.3");

    // verify the Authorization header was sent
    let seen = environment.take_remote_file_auth("https://dprint.example.com/@dprint/foo");
    assert_eq!(seen.as_deref(), Some("Bearer MYTOKEN"));
  }

  #[test]
  fn test_get_packument_url_scoped() {
    assert_eq!(
      get_packument_url("https://registry.npmjs.org", "@dprint/typescript"),
      "https://registry.npmjs.org/@dprint/typescript"
    );
  }

  #[test]
  fn test_get_packument_url_unscoped() {
    assert_eq!(
      get_packument_url("https://registry.npmjs.org", "dprint-plugin-foo"),
      "https://registry.npmjs.org/dprint-plugin-foo"
    );
  }

  #[test]
  fn test_get_tarball_url_from_packument() {
    let packument = serde_json::json!({
      "versions": {
        "0.23.0": {
          "dist": {
            "tarball": "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.23.0.tgz"
          }
        }
      }
    });
    let result = get_tarball_url_from_packument(&packument, "0.23.0", "@dprint/typescript").unwrap();
    assert_eq!(result, "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.23.0.tgz");
  }

  #[test]
  fn test_get_tarball_url_version_not_found() {
    let packument = serde_json::json!({
      "versions": {}
    });
    let result = get_tarball_url_from_packument(&packument, "0.23.0", "@dprint/typescript");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Version 0.23.0 not found"));
  }

  #[tokio::test]
  async fn fetch_npm_latest_info_wasm_skips_tarball_download() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    // intentionally NOT adding the tarball — wasm should not fetch it

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let info = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &specifier,
        start_dir: None,
        want_tarball_sha: false,
      },
      &environment,
    )
    .await
    .unwrap();
    assert_eq!(info.version, "1.2.3");
    assert!(info.tarball_sha256.is_none());
  }

  #[tokio::test]
  async fn fetch_npm_latest_info_wasm_with_want_tarball_sha_fetches_tarball() {
    // wasm plugin updates that previously carried a checksum must get a fresh
    // tarball sha rather than reusing the stale one from the old specifier.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "1.2.3" },
      "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-1.2.3.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = vec![1u8, 2, 3, 4];
    let expected = get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-1.2.3.tgz", tarball_bytes);

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let info = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &specifier,
        start_dir: None,
        want_tarball_sha: true,
      },
      &environment,
    )
    .await
    .unwrap();
    assert_eq!(info.version, "1.2.3");
    assert_eq!(info.tarball_sha256.as_deref(), Some(expected.as_str()));
  }

  #[tokio::test]
  async fn fetch_npm_latest_info_process_downloads_tarball_for_checksum() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "2.0.0" },
      "versions": { "2.0.0": { "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo-2.0.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo", packument.to_string().into_bytes());
    let tarball_bytes = vec![0u8, 1, 2, 3, 4, 5];
    let expected_checksum = get_sha256_checksum(&tarball_bytes);
    environment.add_remote_file_bytes("https://registry.npmjs.org/foo/-/foo-2.0.0.tgz", tarball_bytes);

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.json".to_string(),
    };
    let info = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &specifier,
        start_dir: None,
        want_tarball_sha: false,
      },
      &environment,
    )
    .await
    .unwrap();
    assert_eq!(info.version, "2.0.0");
    assert_eq!(info.tarball_sha256.as_deref(), Some(expected_checksum.as_str()));
  }

  #[test]
  fn test_extract_tarball_to_dir() {
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_tarball(&[("package/plugin.wasm", b"wasm-bytes"), ("package/extra/data.bin", b"extra-data")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        extract_tarball_to_dir(&tarball, &dest, &env).unwrap();

        assert_eq!(std::fs::read(dest.join("plugin.wasm")).unwrap(), b"wasm-bytes");
        assert_eq!(std::fs::read(dest.join("extra").join("data.bin")).unwrap(), b"extra-data");
        // the "package" prefix should be stripped
        assert!(!dest.join("package").exists());
      })
    });
  }

  #[test]
  fn extract_tarball_rejects_inconsistent_wrapper() {
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_tarball(&[("package/plugin.wasm", b"wasm-bytes"), ("other/extra.bin", b"stray")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        let err = extract_tarball_to_dir(&tarball, &dest, &env).unwrap_err();
        assert!(err.to_string().contains("Inconsistent npm tarball"), "got: {}", err);
      })
    });
  }

  #[test]
  fn extract_tarball_rejects_root_only_entries() {
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        // a single file with no wrapper directory — would have been silently dropped
        let tarball = create_test_tarball(&[("plugin.wasm", b"wasm-bytes")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        let err = extract_tarball_to_dir(&tarball, &dest, &env).unwrap_err();
        assert!(err.to_string().contains("no extractable files"), "got: {}", err);
      })
    });
  }

  #[test]
  fn extract_tarball_accepts_leading_curdir_components() {
    // GNU tar's default output prefixes every entry with `./`. The wrapper-
    // detection logic must look past those leading `CurDir`s rather than
    // treating them as the first directory.
    use crate::environment::RealEnvironment;
    use crate::test_helpers::create_test_npm_tarball_raw_paths;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_npm_tarball_raw_paths(&[("./package/plugin.wasm", b"wasm-bytes"), ("./package/extra.bin", b"extra")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        extract_tarball_to_dir(&tarball, &dest, &env).unwrap();
        assert_eq!(std::fs::read(dest.join("plugin.wasm")).unwrap(), b"wasm-bytes");
        assert_eq!(std::fs::read(dest.join("extra.bin")).unwrap(), b"extra");
      })
    });
  }

  #[test]
  fn extract_tarball_rejects_absolute_path_entries() {
    // a tarball entry whose first component is `/` would otherwise pin the
    // wrapper to "/" and write the entry's tail under output_dir, mis-shaping
    // the extract. Reject before that mistake can take effect.
    use crate::environment::RealEnvironment;
    use crate::test_helpers::create_test_npm_tarball_raw_paths;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_npm_tarball_raw_paths(&[("/etc/passwd", b"pwned")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        let err = extract_tarball_to_dir(&tarball, &dest, &env).unwrap_err();
        assert!(err.to_string().contains("non-relative top-level component"), "got: {}", err);
      })
    });
  }

  use crate::test_helpers::create_test_npm_tarball as create_test_tarball;

  #[test]
  fn registry_dir_segment_uses_host_for_normal_urls() {
    assert_eq!(registry_dir_segment("https://registry.npmjs.org"), "registry.npmjs.org");
    assert_eq!(registry_dir_segment("https://registry.npmjs.org/"), "registry.npmjs.org");
    // non-default ports are encoded so :443 vs :8443 don't share a directory
    assert_eq!(registry_dir_segment("http://localhost:8080"), "localhost_8080");
  }

  #[test]
  fn registry_dir_segment_unparseable_urls_get_distinct_hashed_segments() {
    // distinct unparseable URLs must NOT share the literal "unknown" — a
    // collision there would silently mix two registries' tarballs in one dir.
    let a = registry_dir_segment("not a url at all");
    let b = registry_dir_segment("also not a url");
    assert!(a.starts_with("unknown_"), "got: {a}");
    assert!(b.starts_with("unknown_"), "got: {b}");
    assert_ne!(a, b);

    // deterministic: same input → same segment
    assert_eq!(a, registry_dir_segment("not a url at all"));
  }

  #[test]
  fn registry_dir_segment_hostless_urls_get_distinct_hashed_segments() {
    // `file:` URLs parse fine but have no host — historically also "unknown"
    let a = registry_dir_segment("file:///tmp/registry-a");
    let b = registry_dir_segment("file:///tmp/registry-b");
    assert!(a.starts_with("unknown_"), "got: {a}");
    assert!(b.starts_with("unknown_"), "got: {b}");
    assert_ne!(a, b);
  }

  #[test]
  fn extract_tarball_is_idempotent_when_dest_dir_exists() {
    // a second call must not re-extract; trusting that name@version is
    // immutable means we can fast-path-skip if dest_dir already exists.
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_tarball(&[("package/plugin.wasm", b"first-extract")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        extract_tarball_to_dir(&tarball, &dest, &env).unwrap();
        assert_eq!(std::fs::read(dest.join("plugin.wasm")).unwrap(), b"first-extract");

        // a second call with different bytes must NOT overwrite — we trust
        // dest_dir's existence to mean "already extracted"
        let different = create_test_tarball(&[("package/plugin.wasm", b"second-extract")]);
        extract_tarball_to_dir(&different, &dest, &env).unwrap();
        assert_eq!(
          std::fs::read(dest.join("plugin.wasm")).unwrap(),
          b"first-extract",
          "dest_dir should not be re-extracted"
        );

        // and there shouldn't be any leftover temp dirs
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
          .unwrap()
          .filter_map(|e| e.ok())
          .map(|e| e.file_name())
          .filter(|name| name.to_string_lossy().contains(".tmp"))
          .collect();
        assert!(leftover.is_empty(), "expected no .tmp leftover, got {leftover:?}");
      })
    });
  }

  #[test]
  fn extract_tarball_fast_paths_when_dest_dir_exists_from_a_prior_extract() {
    // dest_dir is already populated (e.g. from a previous extract or a
    // concurrent winner that finished before we even started). The initial
    // path_exists check must short-circuit and leave the existing contents
    // alone.
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_tarball(&[("package/plugin.wasm", b"second-attempt")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("plugin.wasm"), b"winner").unwrap();

        extract_tarball_to_dir(&tarball, &dest, &env).unwrap();
        assert_eq!(std::fs::read(dest.join("plugin.wasm")).unwrap(), b"winner");

        // no temp dir orphans
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
          .unwrap()
          .filter_map(|e| e.ok())
          .map(|e| e.file_name())
          .filter(|name| name.to_string_lossy().contains(".tmp"))
          .collect();
        assert!(leftover.is_empty(), "expected no .tmp leftover, got {leftover:?}");
      })
    });
  }

  /// Test-only seam to exercise the rename-race fallback in
  /// `extract_tarball_to_dir`. Production has no business calling this — the
  /// fast-path skip on existing `dest_dir` is part of the correctness story
  /// — so it lives here under `#[cfg(test)]` rather than as a module-private
  /// function that future production code could pick up by accident.
  fn extract_tarball_skipping_existence_check<E: Environment>(tarball_bytes: &[u8], dest_dir: &Path, environment: &E) -> Result<()> {
    use crate::utils::fs::get_atomic_path;
    let temp_dir = get_atomic_path(environment, dest_dir);
    environment.mk_dir_all(&temp_dir)?;
    if let Err(err) = extract_tarball_to_dir_inner(tarball_bytes, &temp_dir, environment) {
      let _ = environment.remove_dir_all(&temp_dir);
      return Err(err);
    }
    match environment.rename(&temp_dir, dest_dir) {
      Ok(()) => Ok(()),
      Err(err) => {
        if environment.path_exists(dest_dir) {
          let _ = environment.remove_dir_all(&temp_dir);
          Ok(())
        } else {
          let _ = environment.remove_dir_all(&temp_dir);
          Err(err.into())
        }
      }
    }
  }

  #[test]
  fn extract_tarball_falls_back_when_rename_loses_to_a_concurrent_extract() {
    // exercise the path where the initial path_exists check returned false
    // but dest_dir is populated by another extract before our rename runs.
    // we call the test seam directly to skip the fast-path check and pre-
    // populate dest_dir so the rename collides — the function must accept
    // the winner and clean up its own temp dir rather than erroring.
    use crate::environment::RealEnvironment;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_tarball(&[("package/plugin.wasm", b"loser")]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        // a winning concurrent extract has populated dest_dir with content.
        // a non-empty dest is what makes rename of our temp dir collide.
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("plugin.wasm"), b"winner").unwrap();

        extract_tarball_skipping_existence_check(&tarball, &dest, &env).unwrap();
        // winner's contents preserved
        assert_eq!(std::fs::read(dest.join("plugin.wasm")).unwrap(), b"winner");

        // our temp dir was cleaned up
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
          .unwrap()
          .filter_map(|e| e.ok())
          .map(|e| e.file_name())
          .filter(|name| name.to_string_lossy().contains(".tmp"))
          .collect();
        assert!(leftover.is_empty(), "expected no .tmp leftover, got {leftover:?}");
      })
    });
  }

  #[tokio::test]
  async fn download_with_auth_keeps_header_on_same_origin_redirect() {
    use crate::environment::TestEnvironment;
    use crate::environment::UrlDownloader;
    let environment = TestEnvironment::new();
    let start = "https://registry.example.com/foo";
    let redirected = "https://registry.example.com/foo/latest";
    environment.add_remote_file_redirect(start, redirected);
    environment.add_remote_file_bytes(redirected, b"ok".to_vec());

    let url = url::Url::parse(start).unwrap();
    let _ = environment.download_file_with_auth_err_404(&url, Some("Bearer T")).await.unwrap();

    assert_eq!(environment.take_remote_file_auth(start).as_deref(), Some("Bearer T"));
    assert_eq!(environment.take_remote_file_auth(redirected).as_deref(), Some("Bearer T"));
  }

  #[tokio::test]
  async fn download_with_auth_drops_header_on_cross_origin_redirect() {
    use crate::environment::TestEnvironment;
    use crate::environment::UrlDownloader;
    let environment = TestEnvironment::new();
    let start = "https://registry.example.com/foo";
    let cdn = "https://cdn.example.net/foo.tgz";
    environment.add_remote_file_redirect(start, cdn);
    environment.add_remote_file_bytes(cdn, b"tarball".to_vec());

    let url = url::Url::parse(start).unwrap();
    let _ = environment.download_file_with_auth_err_404(&url, Some("Bearer T")).await.unwrap();

    // initial registry request gets the token; CDN does not
    assert_eq!(environment.take_remote_file_auth(start).as_deref(), Some("Bearer T"));
    assert_eq!(environment.take_remote_file_auth(cdn), None);
  }

  #[tokio::test]
  async fn resolve_npm_from_registry_sends_auth_on_packument_and_tarball() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "versions": {
        "1.0.0": { "dist": { "tarball": "https://private.example.com/foo/-/foo-1.0.0.tgz" } }
      }
    });
    environment.add_remote_file_bytes("https://private.example.com/foo", packument.to_string().into_bytes());
    let tarball = create_test_tarball(&[("package/plugin.wasm", b"wasm")]);
    let tarball_checksum = get_sha256_checksum(&tarball);
    environment.add_remote_file_bytes("https://private.example.com/foo/-/foo-1.0.0.tgz", tarball);

    let registry = NpmRegistryResolution {
      url: "https://private.example.com".to_string(),
      auth_header: Some("Bearer SECRET".to_string()),
    };
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let _ = resolve_npm_from_registry(&specifier, Some(&tarball_checksum), &registry, None, &environment)
      .await
      .unwrap();

    assert_eq!(
      environment.take_remote_file_auth("https://private.example.com/foo").as_deref(),
      Some("Bearer SECRET")
    );
    assert_eq!(
      environment.take_remote_file_auth("https://private.example.com/foo/-/foo-1.0.0.tgz").as_deref(),
      Some("Bearer SECRET")
    );
  }

  #[tokio::test]
  async fn resolve_npm_from_registry_drops_auth_on_cross_origin_tarball() {
    // registry's packument points at a different host (a CDN) for the tarball —
    // the auth header must not be sent to the CDN.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "versions": {
        "1.0.0": { "dist": { "tarball": "https://cdn.example.net/foo-1.0.0.tgz" } }
      }
    });
    environment.add_remote_file_bytes("https://private.example.com/foo", packument.to_string().into_bytes());
    let tarball = create_test_tarball(&[("package/plugin.wasm", b"wasm")]);
    let tarball_checksum = get_sha256_checksum(&tarball);
    environment.add_remote_file_bytes("https://cdn.example.net/foo-1.0.0.tgz", tarball);

    let registry = NpmRegistryResolution {
      url: "https://private.example.com".to_string(),
      auth_header: Some("Bearer SECRET".to_string()),
    };
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: Some("1.0.0".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let _ = resolve_npm_from_registry(&specifier, Some(&tarball_checksum), &registry, None, &environment)
      .await
      .unwrap();

    assert_eq!(
      environment.take_remote_file_auth("https://private.example.com/foo").as_deref(),
      Some("Bearer SECRET")
    );
    // tarball request is to a different origin — no credentials leak
    assert_eq!(environment.take_remote_file_auth("https://cdn.example.net/foo-1.0.0.tgz"), None);
  }

  /// Stages an npm registry tarball at the default registry so
  /// `try_resolve_process_plugin_per_platform_binary` can fetch it. Returns
  /// the tarball's SHA-256 so callers can plug it into plugin.json.
  fn stage_per_platform_npm_package(environment: &crate::environment::TestEnvironment, name: &str, version: &str, files: &[(&str, &[u8])]) -> String {
    let tarball = create_test_tarball(files);
    let checksum = get_sha256_checksum(&tarball);
    let packument = serde_json::json!({
      "versions": {
        version: { "dist": { "tarball": format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz") } }
      }
    });
    environment.add_remote_file_bytes(&format!("https://registry.npmjs.org/{name}"), packument.to_string().into_bytes());
    environment.add_remote_file_bytes(&format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"), tarball);
    checksum
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_aarch64_falls_back_to_x86_64() {
    // a process plugin manifest that only ships linux-x86_64 should still
    // resolve on linux/aarch64 — matches the HTTPS path's behavior.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.set_os("linux");
    environment.set_cpu_arch("aarch64");

    // the per-platform package ships the executable inside its tarball.
    // The checksum in plugin.json is the tarball's SHA-256, not the binary's.
    let tarball_checksum = stage_per_platform_npm_package(&environment, "foo-linux-x86_64", "1.0.0", &[("package/foo", b"fake-binary-contents")]);

    // the manifest only lists linux-x86_64; the aarch64 host should still resolve it
    let manifest = serde_json::json!({
      "schemaVersion": 2,
      "name": "foo",
      "version": "1.0.0",
      "linux-x86_64": {
        "reference": "npm:foo-linux-x86_64@1.0.0/foo",
        "checksum": tarball_checksum,
      },
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let resolved = resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment)
      .await
      .unwrap();
    let tarball = resolved.pre_resolved_tarball.expect("process plugin should have a pre-resolved tarball");
    assert_eq!(tarball.name, "foo");
    assert_eq!(tarball.version, "1.0.0");
    assert_eq!(tarball.executable_sub_path, "foo");
    // tarball bytes are passed straight through (verified against the
    // checksum); we don't extract here, so we just sanity-check non-emptiness.
    assert!(!tarball.tarball_bytes.is_empty());
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_rejects_unversioned_reference() {
    // plugin.json must pin a version for its per-platform npm reference —
    // we need a specific tarball to verify against the checksum.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.set_os("linux");
    environment.set_cpu_arch("x86_64");

    let manifest = serde_json::json!({
      "schemaVersion": 2,
      "name": "foo",
      "version": "1.0.0",
      "linux-x86_64": {
        "reference": "npm:foo-linux-x86_64/foo",
        "checksum": "0".repeat(64),
      },
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("must include a version"), "got: {msg}");
    assert!(msg.contains("npm:foo-linux-x86_64/foo"), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_rejects_tarball_checksum_mismatch() {
    // plugin.json's checksum doesn't match the fetched tarball — surface a
    // clear "invalid checksum" error so the plugin author / user can tell
    // what broke.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.set_os("linux");
    environment.set_cpu_arch("x86_64");

    let _real_checksum = stage_per_platform_npm_package(&environment, "foo-linux-x86_64", "1.0.0", &[("package/foo", b"binary")]);
    let bogus_checksum = "0".repeat(64);

    let manifest = serde_json::json!({
      "schemaVersion": 2,
      "name": "foo",
      "version": "1.0.0",
      "linux-x86_64": {
        "reference": "npm:foo-linux-x86_64@1.0.0/foo",
        "checksum": bogus_checksum,
      },
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("Invalid checksum"), "got: {msg}");
    assert!(msg.contains("foo-linux-x86_64"), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_rejects_bad_schema_version() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let manifest = serde_json::json!({
      "schemaVersion": 1,
      "name": "foo",
      "version": "1.0.0",
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    assert!(
      err.to_string().contains("schema version") || format!("{err:#}").contains("schema version"),
      "expected schema-version error, got: {err:#}"
    );
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_rejects_https_reference() {
    // an npm-installed process plugin can't reference its platform binary
    // over the network — the whole point of installing via npm is to avoid
    // surprise HTTP fetches at format time.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.set_os("linux");
    environment.set_cpu_arch("x86_64");
    let manifest = serde_json::json!({
      "schemaVersion": 2,
      "name": "foo",
      "version": "1.0.0",
      "linux-x86_64": {
        "reference": "https://example.com/foo-linux-x86_64.zip",
        "checksum": "deadbeef",
      },
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("Network references aren't allowed"), "got: {msg}");
    assert!(msg.contains("https://example.com/foo-linux-x86_64.zip"), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_registry_suggests_plugin_json_when_wasm_missing() {
    // user wrote `npm:@dprint/exec@0.6.2` (defaulting to plugin.wasm) but the
    // package ships a plugin.json instead. The error should tell them to
    // reference plugin.json, not just "Is the package a dprint plugin?".
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "versions": { "0.6.2": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/exec/-/exec-0.6.2.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/exec", packument.to_string().into_bytes());
    let tarball = create_test_tarball(&[("package/plugin.json", br#"{"schemaVersion":2,"name":"exec","version":"0.6.2"}"#)]);
    let tarball_checksum = get_sha256_checksum(&tarball);
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/exec/-/exec-0.6.2.tgz", tarball);

    let registry = NpmRegistryResolution {
      url: "https://registry.npmjs.org".to_string(),
      auth_header: None,
    };
    let specifier = NpmSpecifier {
      name: "@dprint/exec".to_string(),
      version: Some("0.6.2".to_string()),
      path: "plugin.wasm".to_string(),
    };
    let err = match resolve_npm_from_registry(&specifier, Some(&tarball_checksum), &registry, None, &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("plugin.json instead"), "got: {msg}");
    assert!(msg.contains("npm:@dprint/exec@0.6.2/plugin.json"), "got: {msg}");
  }

  #[test]
  fn find_npm_plugin_local_path_suggests_plugin_json_when_wasm_missing() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/node_modules/@dprint/exec").unwrap();
    environment
      .write_file(
        "/node_modules/@dprint/exec/plugin.json",
        r#"{"schemaVersion":2,"name":"exec","version":"0.6.2"}"#,
      )
      .unwrap();

    let specifier = NpmSpecifier {
      name: "@dprint/exec".to_string(),
      version: None,
      path: "plugin.wasm".to_string(),
    };
    let err = find_npm_plugin_local_path(&specifier, std::path::Path::new("/"), &environment).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("plugin.json instead"), "got: {msg}");
    // unversioned suggestion should not pin a version
    assert!(msg.contains("npm:@dprint/exec/plugin.json"), "got: {msg}");
  }

  #[test]
  fn find_npm_plugin_local_path_no_alternate_when_neither_present() {
    // package directory exists but contains nothing useful — falls back to
    // the generic "Is the package a dprint plugin?" message.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/README.md", "hi").unwrap();

    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.wasm".to_string(),
    };
    let err = find_npm_plugin_local_path(&specifier, std::path::Path::new("/"), &environment).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("Is the package a dprint plugin?"), "got: {msg}");
    assert!(!msg.contains("instead"), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_missing_package_suggests_versioned_specifier_wasm() {
    // user wrote `npm:@dprint/typescript` but hasn't run `npm install`. The
    // error should not just say "npm install" — it should also offer a
    // ready-to-paste `npm:<name>@<version>` alternative. For wasm plugins we
    // skip the checksum (it's optional and would be noise).
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "0.99.0" },
      "versions": { "0.99.0": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/typescript/-/typescript-0.99.0.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/typescript", packument.to_string().into_bytes());

    let specifier = NpmSpecifier {
      name: "@dprint/typescript".to_string(),
      version: None,
      path: "plugin.wasm".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("Could not find @dprint/typescript in node_modules"), "got: {msg}");
    assert!(msg.contains("npm install @dprint/typescript"), "got: {msg}");
    // suggestion includes the resolved latest version, no checksum for wasm
    assert!(msg.contains("npm:@dprint/typescript@0.99.0"), "got: {msg}");
    assert!(!msg.contains("npm:@dprint/typescript@0.99.0@"), "wasm shouldn't carry a checksum: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_missing_package_suggests_versioned_specifier_process() {
    // for process plugins (path is plugin.json), keep the path component in
    // the suggestion. The checksum requirement is surfaced by the next
    // resolve step (which quotes the exact SHA to use), so we don't bake it
    // into this message.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let packument = serde_json::json!({
      "dist-tags": { "latest": "0.6.2" },
      "versions": { "0.6.2": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/exec/-/exec-0.6.2.tgz" } } }
    });
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/exec", packument.to_string().into_bytes());
    // intentionally no tarball mock — confirms we don't fetch it for the suggestion

    let specifier = NpmSpecifier {
      name: "@dprint/exec".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("Could not find @dprint/exec in node_modules"), "got: {msg}");
    assert!(msg.contains("npm:@dprint/exec@0.6.2/plugin.json"), "got: {msg}");
    // no checksum appended — the next step explains that requirement
    assert!(!msg.contains("npm:@dprint/exec@0.6.2/plugin.json@"), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_missing_package_falls_back_when_registry_unreachable() {
    // the registry is unreachable (no mocks set up) — the error degrades
    // gracefully to the bare "npm install" hint rather than swallowing
    // the original miss with a confusing network error.
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();

    let specifier = NpmSpecifier {
      name: "@dprint/typescript".to_string(),
      version: None,
      path: "plugin.wasm".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("Could not find @dprint/typescript in node_modules"), "got: {msg}");
    assert!(msg.contains("npm install @dprint/typescript"), "got: {msg}");
    // no version suggestion when we couldn't reach the registry
    assert!(!msg.contains("OR specify a version (ex."), "got: {msg}");
  }

  #[tokio::test]
  async fn resolve_npm_from_node_modules_process_plugin_rejects_non_process_kind() {
    use crate::environment::TestEnvironment;
    let environment = TestEnvironment::new();
    let manifest = serde_json::json!({
      "schemaVersion": 2,
      "kind": "other",
      "name": "foo",
      "version": "1.0.0",
    });
    environment.mk_dir_all("/node_modules/foo").unwrap();
    environment.write_file("/node_modules/foo/plugin.json", &manifest.to_string()).unwrap();
    let specifier = NpmSpecifier {
      name: "foo".to_string(),
      version: None,
      path: "plugin.json".to_string(),
    };
    let err = match resolve_npm_from_node_modules(&specifier, std::path::Path::new("/"), &environment).await {
      Ok(_) => panic!("expected an error"),
      Err(e) => e,
    };
    assert!(
      format!("{err:#}").contains("Unsupported plugin kind: other"),
      "expected unsupported-kind error, got: {err:#}"
    );
  }

  #[cfg(unix)]
  #[test]
  fn extract_tarball_preserves_exec_bits_via_env_set_permissions() {
    use crate::environment::RealEnvironment;
    use crate::test_helpers::create_test_npm_tarball_with_modes;
    use std::os::unix::fs::PermissionsExt;
    RealEnvironment::run_test_with_real_env(|env| {
      Box::pin(async move {
        let tarball = create_test_npm_tarball_with_modes(&[("package/plugin.wasm", b"wasm", 0o644), ("package/scripts/run.sh", b"#!/bin/sh\n", 0o755)]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("extracted");

        extract_tarball_to_dir(&tarball, &dest, &env).unwrap();

        let exec_mode = std::fs::metadata(dest.join("scripts").join("run.sh")).unwrap().permissions().mode() & 0o777;
        assert_eq!(exec_mode, 0o755, "expected exec bits preserved");
        let plain_mode = std::fs::metadata(dest.join("plugin.wasm")).unwrap().permissions().mode() & 0o777;
        // 0o644 is the default — set_permissions is skipped for it
        assert_eq!(plain_mode, 0o644);
      })
    });
  }
}
