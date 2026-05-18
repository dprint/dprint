use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use flate2::read::GzDecoder;
use tar::Archive;

use crate::environment::Environment;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::deno_npmrc;
use crate::utils::deno_npmrc::RegistryConfig;
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
  /// For node-resolved process plugins, the pre-resolved platform binary zip bytes.
  /// When set, setup_process_plugin should use these directly instead of
  /// downloading from the reference URL in plugin.json.
  pub pre_resolved_zip: Option<ProcessPluginZipBytes>,
}

/// Pre-resolved zip bytes for a process plugin's platform-specific binary.
pub struct ProcessPluginZipBytes {
  pub name: String,
  pub version: String,
  pub zip_bytes: Vec<u8>,
}

/// Information about the latest published version of an npm-distributed plugin.
pub struct NpmLatestInfo {
  pub version: String,
  /// SHA-256 of the latest tarball, computed only for non-wasm plugins (where
  /// a checksum is required in the dprint.json specifier). `None` for wasm.
  pub tarball_sha256: Option<String>,
}

/// Fetches the latest version of an npm-distributed plugin (and its tarball
/// checksum for non-wasm plugins). Used by `dprint config update`.
pub async fn fetch_npm_latest_info(specifier: &NpmSpecifier, start_dir: Option<&Path>, environment: &impl Environment) -> Result<NpmLatestInfo> {
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

  let tarball_sha256 = if specifier.plugin_kind() != PluginKind::Wasm {
    let tarball_url_str = get_tarball_url_from_packument(&packument, &latest_version, &specifier.name)?;
    let tarball_url = url::Url::parse(&tarball_url_str).with_context(|| format!("Failed to parse npm tarball URL: {}", tarball_url_str))?;
    let (_, tarball_file) = environment
      .download_file_with_auth_err_404(&tarball_url, registry.auth_header.as_deref())
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
pub async fn resolve_npm_from_registry(
  specifier: &NpmSpecifier,
  checksum: Option<&str>,
  registry: &NpmRegistryResolution,
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

  // download the tarball
  let (_, tarball_file) = environment
    .download_file_with_auth_err_404(&tarball_url, registry.auth_header.as_deref())
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
  let package_name = specifier.name.clone();
  let environment = environment.clone();
  let (plugin_bytes, local_path) = dprint_core::async_runtime::spawn_blocking(move || -> Result<_> {
    extract_tarball_to_dir(&tarball_bytes, &extract_dir, &environment)?;

    let plugin_file_path = extract_dir.join(&plugin_path);
    if !environment.path_exists(&plugin_file_path) {
      bail!(
        "Could not find {} in npm package {}. Is the package a dprint plugin?",
        plugin_path,
        package_name,
      );
    }
    let plugin_bytes = environment
      .read_file_bytes(&plugin_file_path)
      .with_context(|| format!("Failed to read {}", plugin_file_path.display()))?;
    let canonical = environment.canonicalize(&plugin_file_path)?;
    Ok((plugin_bytes, PathSource::new_local(canonical)))
  })
  .await??;

  Ok(NpmResolvedPlugin {
    plugin_bytes,
    plugin_kind,
    local_path,
    pre_resolved_zip: None,
  })
}

/// Resolves an npm plugin from node_modules (unversioned specifier).
pub fn resolve_npm_from_node_modules(specifier: &NpmSpecifier, config_dir: &Path, environment: &impl Environment) -> Result<NpmResolvedPlugin> {
  let local_path = find_npm_plugin_local_path(specifier, config_dir, environment)?;
  let canonical = local_path.maybe_local_path().expect("find_npm_plugin_local_path returns Local").clone();
  let plugin_bytes = environment
    .read_file_bytes(canonical.as_ref())
    .with_context(|| format!("Failed to read {}", canonical.display()))?;

  // for process plugins, resolve the platform-specific binary from node_modules too
  let pre_resolved_zip = if specifier.plugin_kind() == PluginKind::Process {
    Some(resolve_process_plugin_dep_from_node_modules(&plugin_bytes, config_dir, environment)?)
  } else {
    None
  };

  Ok(NpmResolvedPlugin {
    plugin_bytes,
    plugin_kind: specifier.plugin_kind(),
    local_path,
    pre_resolved_zip,
  })
}

/// Locates the canonical local path an unversioned npm specifier resolves to,
/// without reading the plugin file or doing process-plugin dep resolution.
pub fn find_npm_plugin_local_path(specifier: &NpmSpecifier, config_dir: &Path, environment: &impl Environment) -> Result<PathSource> {
  let package_dir = find_package_in_node_modules(&specifier.name, config_dir, environment)?;
  let plugin_path = package_dir.join(&specifier.path);

  if !environment.path_exists(&plugin_path) {
    bail!(
      "Could not find {} in {}. Is the package a dprint plugin?",
      specifier.path,
      package_dir.display()
    );
  }

  let canonical = environment.canonicalize(&plugin_path)?;
  Ok(PathSource::new_local(canonical))
}

/// Reads a process plugin manifest (plugin.json), finds the platform-specific
/// reference, and if it's an npm specifier, resolves it from node_modules.
fn resolve_process_plugin_dep_from_node_modules(plugin_json_bytes: &[u8], config_dir: &Path, environment: &impl Environment) -> Result<ProcessPluginZipBytes> {
  let plugin_file: serde_json::Value = serde_json::from_slice(plugin_json_bytes).context("Failed to parse process plugin manifest (plugin.json)")?;
  let name = plugin_file
    .get("name")
    .and_then(|v| v.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing 'name' in plugin.json"))?
    .to_string();
  let version = plugin_file
    .get("version")
    .and_then(|v| v.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing 'version' in plugin.json"))?
    .to_string();

  let (reference, checksum) = get_os_reference_and_checksum(&plugin_file, environment)?;

  // parse as npm specifier — use the package name for node_modules resolution
  if !reference.starts_with("npm:") {
    bail!("Expected an npm: reference in plugin.json for node_modules resolution, but got: {}", reference);
  }
  let parsed = crate::utils::parse_npm_specifier(&reference)?;
  let dep_name = &parsed.specifier.name;

  let dep_dir = find_package_in_node_modules(dep_name, config_dir, environment)?;

  // the npm reference in plugin.json names the zip exactly (e.g.
  // `npm:@scope/foo-linux-x64@1.0.0/plugin.zip`), so look it up by path
  // rather than scanning the directory for an arbitrary .zip
  let zip_path = dep_dir.join(&parsed.specifier.path);
  if !environment.path_exists(&zip_path) {
    bail!(
      "Could not find {} in {}. The plugin.json reference does not match the installed package contents.",
      parsed.specifier.path,
      dep_dir.display(),
    );
  }
  let zip_bytes = environment
    .read_file_bytes(&zip_path)
    .with_context(|| format!("Failed to read {}", zip_path.display()))?;

  verify_sha256_checksum(&zip_bytes, &checksum).with_context(|| {
    format!(
      "Invalid checksum for process plugin dependency '{}'. This is likely a bug in the process plugin.",
      dep_name,
    )
  })?;

  Ok(ProcessPluginZipBytes { name, version, zip_bytes })
}

fn get_os_reference_and_checksum(plugin_file: &serde_json::Value, environment: &impl Environment) -> Result<(String, String)> {
  let arch = environment.cpu_arch();
  let os = environment.os();
  let key = match os.as_str() {
    "linux" => match arch.as_str() {
      "x86_64" => "linux-x86_64",
      "aarch64" => "linux-aarch64",
      "riscv64" => "linux-riscv64",
      "loongarch64" => "linux-loongarch64",
      _ => bail!("Unsupported CPU architecture: {} ({})", arch, os),
    },
    "linux-musl" => match arch.as_str() {
      "x86_64" => "linux-x86_64-musl",
      "aarch64" => "linux-aarch64-musl",
      "riscv64" => "linux-riscv64-musl",
      "loongarch64" => "linux-loongarch64-musl",
      _ => bail!("Unsupported CPU architecture: {} ({})", arch, os),
    },
    "macos" => match arch.as_str() {
      "x86_64" => "darwin-x86_64",
      "aarch64" => "darwin-aarch64",
      _ => bail!("Unsupported CPU architecture: {} ({})", arch, os),
    },
    "windows" => match arch.as_str() {
      "x86_64" => "windows-x86_64",
      "aarch64" => "windows-aarch64",
      _ => bail!("Unsupported CPU architecture: {} ({})", arch, os),
    },
    _ => bail!("Unsupported operating system: {}", os),
  };

  let entry = plugin_file
    .get(key)
    .and_then(|v| v.as_object())
    .ok_or_else(|| anyhow::anyhow!("No entry for platform '{}' in plugin.json", key))?;
  let reference = entry
    .get("reference")
    .and_then(|v| v.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing 'reference' for platform '{}' in plugin.json", key))?
    .to_string();
  let checksum = entry
    .get("checksum")
    .and_then(|v| v.as_str())
    .ok_or_else(|| anyhow::anyhow!("Missing 'checksum' for platform '{}' in plugin.json", key))?
    .to_string();
  Ok((reference, checksum))
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
/// (and port, if non-default). Falls back to "unknown" if the URL is unparseable.
pub(super) fn registry_dir_segment(registry_url: &str) -> String {
  let Ok(url) = url::Url::parse(registry_url) else {
    return "unknown".to_string();
  };
  let Some(host) = url.host_str() else {
    return "unknown".to_string();
  };
  match url.port() {
    Some(port) => format!("{host}_{port}"),
    None => host.to_string(),
  }
}

/// Extracts an npm tarball to a directory on disk.
/// Uses a .temp directory and rename for reliability.
/// Strips the first path component (usually `package/`) from each entry.
fn extract_tarball_to_dir(tarball_bytes: &[u8], dest_dir: &Path, environment: &impl Environment) -> Result<()> {
  let temp_dir = dest_dir.with_extension("temp");

  // clean up any previous failed extraction
  let _ = environment.remove_dir_all(&temp_dir);
  environment.mk_dir_all(&temp_dir)?;

  let result = extract_tarball_to_dir_inner(tarball_bytes, &temp_dir, environment);
  if result.is_err() {
    // clean up on failure
    let _ = environment.remove_dir_all(&temp_dir);
    return result;
  }

  // atomic-ish rename: remove old dir, rename temp to final
  let _ = environment.remove_dir_all(dest_dir);
  environment.rename(&temp_dir, dest_dir)?;

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

    let mut components = path.components();
    let Some(first) = components.next() else {
      continue;
    };
    let first_os = first.as_os_str().to_os_string();
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
  if let Some(home_dir) = environment.get_home_dir() {
    if let Some(info) = resolve_registry_from_npmrc(package_name, &home_dir.join(".npmrc"), environment) {
      return info;
    }
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
  let auth_header = compute_auth_header(resolved.get_registry_config(package_name).as_ref());

  Some(NpmRegistryResolution { url, auth_header })
}

/// Returns the scope (without the `@`) for a scoped package, or `None` for
/// unscoped packages.
fn scope_of(package_name: &str) -> Option<&str> {
  package_name.strip_prefix('@')?.split_once('/').map(|(scope, _)| scope)
}

/// Builds an HTTP `Authorization` header value from a registry's `.npmrc`
/// credentials. Supports `_authToken` (Bearer) and `_auth` (Basic). The
/// `username` / `_password` combination is not yet supported (would require
/// base64-encoding) — returns `None` and emits no header in that case.
fn compute_auth_header(config: &RegistryConfig) -> Option<String> {
  if let Some(token) = &config.auth_token {
    return Some(format!("Bearer {}", token));
  }
  if let Some(auth) = &config.auth {
    return Some(format!("Basic {}", auth));
  }
  None
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
fn find_package_in_node_modules(package_name: &str, start_dir: &Path, environment: &impl Environment) -> Result<std::path::PathBuf> {
  for dir in start_dir.ancestors() {
    let candidate = dir.join("node_modules").join(package_name);
    if environment.path_exists(&candidate) {
      return Ok(candidate);
    }
  }

  bail!(
    "Could not find {} in node_modules. Make sure the package is installed (npm install {}).",
    package_name,
    package_name,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn compute_auth_header_supports_auth_token_and_auth() {
    let mut cfg = RegistryConfig::default();
    assert_eq!(compute_auth_header(&cfg), None);

    cfg.auth_token = Some("tok".to_string());
    assert_eq!(compute_auth_header(&cfg), Some("Bearer tok".to_string()));

    cfg.auth_token = None;
    cfg.auth = Some("dXNlcjpwd2Q=".to_string());
    assert_eq!(compute_auth_header(&cfg), Some("Basic dXNlcjpwd2Q=".to_string()));

    // auth_token wins over auth when both are present
    cfg.auth_token = Some("tok".to_string());
    assert_eq!(compute_auth_header(&cfg), Some("Bearer tok".to_string()));
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
    let info = fetch_npm_latest_info(&specifier, Some(std::path::Path::new("/repo")), &environment).await.unwrap();
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
    let info = fetch_npm_latest_info(&specifier, None, &environment).await.unwrap();
    assert_eq!(info.version, "1.2.3");
    assert!(info.tarball_sha256.is_none());
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
    let info = fetch_npm_latest_info(&specifier, None, &environment).await.unwrap();
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

  use crate::test_helpers::create_test_npm_tarball as create_test_tarball;
}
