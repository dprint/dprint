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
use crate::utils::get_sha256_checksum;
use crate::utils::verify_sha256_checksum;

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

/// Resolves an npm plugin from the registry (versioned specifier).
/// Downloads the tarball, extracts it to the cache directory, and reads the plugin file.
pub async fn resolve_npm_from_registry(
  specifier: &NpmSpecifier,
  checksum: Option<&str>,
  start_dir: Option<&Path>,
  environment: &impl Environment,
) -> Result<NpmResolvedPlugin> {
  let version = specifier
    .version
    .as_deref()
    .ok_or_else(|| anyhow::anyhow!("Cannot resolve npm plugin without a version from the registry"))?;
  let registry_url = get_registry_url(&specifier.name, start_dir, environment);

  // fetch the packument to get the tarball URL
  let packument_url = get_packument_url(&registry_url, &specifier.name);
  log_debug!(environment, "Fetching npm packument: {}", packument_url);
  let packument_bytes = environment
    .download_file_err_404(&packument_url)
    .await
    .with_context(|| format!("Failed to fetch npm packument for {}", specifier.name))?;
  let packument: serde_json::Value =
    serde_json::from_slice(&packument_bytes).with_context(|| format!("Failed to parse npm packument for {}", specifier.name))?;

  let tarball_url = get_tarball_url_from_packument(&packument, version, &specifier.name)?;
  log_debug!(environment, "Downloading npm tarball: {}", tarball_url);

  // download the tarball
  let tarball_bytes = environment
    .download_file_err_404(&tarball_url)
    .await
    .with_context(|| format!("Failed to download npm tarball for {}@{}", specifier.name, version))?;

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
  let extract_dir = get_npm_extract_dir(&specifier.name, version, environment);
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
  let plugin_bytes = environment
    .read_file_bytes(&canonical)
    .with_context(|| format!("Failed to read {}", plugin_path.display()))?;

  // for process plugins, resolve the platform-specific binary from node_modules too
  let pre_resolved_zip = if specifier.plugin_kind() == PluginKind::Process {
    Some(resolve_process_plugin_dep_from_node_modules(&plugin_bytes, config_dir, environment)?)
  } else {
    None
  };

  Ok(NpmResolvedPlugin {
    plugin_bytes,
    plugin_kind: specifier.plugin_kind(),
    local_path: PathSource::new_local(canonical),
    pre_resolved_zip,
  })
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

  // read all files in the dep package directory and zip them, since setup_process_plugin
  // expects zip bytes. Actually — the dep package likely contains the zip itself or the
  // executable directly. Let's look for a zip file or read the directory contents.
  // In practice, the npm platform package contains the zip as the main artifact.
  // Let's look for a .zip file in the package.
  let zip_path = find_zip_in_package(&dep_dir, environment)?;
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

/// Gets the platform-specific reference string from a process plugin manifest.
fn get_os_reference(plugin_file: &serde_json::Value, environment: &impl Environment) -> Result<String> {
  let (reference, _checksum) = get_os_reference_and_checksum(plugin_file, environment)?;
  Ok(reference)
}

/// Gets the platform-specific checksum from a process plugin manifest.
fn get_os_checksum(plugin_file: &serde_json::Value, environment: &impl Environment) -> Result<String> {
  let (_reference, checksum) = get_os_reference_and_checksum(plugin_file, environment)?;
  Ok(checksum)
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
fn find_zip_in_package(package_dir: &Path, environment: &impl Environment) -> Result<PathBuf> {
  let entries = environment.dir_info(package_dir)?;
  for entry in entries {
    if let crate::environment::DirEntry::File { path, .. } = entry {
      if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip")) {
        return Ok(path);
      }
    }
  }
  bail!("Could not find a .zip file in {}", package_dir.display())
}

/// Returns the directory where an npm package tarball should be extracted.
fn get_npm_extract_dir(package_name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  // use a sanitized name for the directory (replace / with __)
  let dir_name = format!("{}@{}", package_name.replace('/', "__"), version);
  environment.get_cache_dir().join("npm").join(dir_name)
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

    // strip the first path component (usually "package" but can also be the package name)
    let mut components = path.components();
    components.next(); // skip first component
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

    // preserve executable permissions on unix
    #[cfg(unix)]
    if let Ok(mode) = entry.header().mode() {
      if mode != 0o644 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(mode))
          .with_context(|| format!("Failed to set permissions on {}", dest_path.display()))?;
      }
    }
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

/// Resolves the npm registry URL for a package, checking (in order):
/// 1. NPM_CONFIG_REGISTRY env var
/// 2. .npmrc files walking up from `start_dir`
/// 3. ~/.npmrc
/// 4. https://registry.npmjs.org
fn get_registry_url(package_name: &str, start_dir: Option<&Path>, environment: &impl Environment) -> String {
  // env vars take precedence over .npmrc
  if let Some(registry) = environment.env_var("NPM_CONFIG_REGISTRY") {
    let registry = registry.to_string_lossy().to_string();
    return registry.trim_end_matches('/').to_string();
  }

  // walk up from the config file's directory checking for .npmrc files
  if let Some(start) = start_dir {
    let mut current = start.to_path_buf();
    loop {
      let npmrc_path = current.join(".npmrc");
      if let Some(url) = resolve_registry_from_npmrc(package_name, &npmrc_path, environment) {
        return url;
      }
      if !current.pop() {
        break;
      }
    }
  }

  // user-level ~/.npmrc
  if let Some(home_dir) = environment.get_home_dir() {
    if let Some(url) = resolve_registry_from_npmrc(package_name, &home_dir.join(".npmrc"), environment) {
      return url;
    }
  }

  deno_npmrc::NPM_DEFAULT_REGISTRY.to_string()
}

/// Parses a single .npmrc file and resolves the registry URL for a package.
/// Returns None if the file doesn't exist or doesn't configure any registries.
fn resolve_registry_from_npmrc(package_name: &str, npmrc_path: &Path, environment: &impl Environment) -> Option<String> {
  let text = environment.read_file(npmrc_path).ok()?;
  let npmrc = deno_npmrc::NpmRc::parse(&text, &|name| environment.env_var(name).map(|v| v.into_string().ok()).flatten()).ok()?;

  // skip this .npmrc if it doesn't configure any registries
  if npmrc.registry.is_none() && npmrc.scope_registries.is_empty() {
    return None;
  }

  let default_url = url::Url::parse(deno_npmrc::NPM_DEFAULT_REGISTRY).unwrap();
  let registry_url = deno_npmrc::NpmRegistryUrl {
    url: default_url,
    from_env: false,
  };
  let resolved = npmrc.as_resolved(&registry_url).ok()?;
  let url = resolved.get_registry_url(package_name);
  Some(url.as_str().trim_end_matches('/').to_string())
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
  let mut current = start_dir.to_path_buf();

  loop {
    let candidate = current.join("node_modules").join(package_name);
    if environment.path_exists(&candidate) {
      return Ok(candidate);
    }

    if !current.pop() {
      break;
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

  fn create_test_tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut tar_builder = tar::Builder::new(Vec::new());
    for (path, contents) in files {
      let mut header = tar::Header::new_gnu();
      header.set_path(path).unwrap();
      header.set_size(contents.len() as u64);
      header.set_mode(0o644);
      header.set_cksum();
      tar_builder.append(&header, *contents).unwrap();
    }
    let tar_bytes = tar_builder.into_inner().unwrap();

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
    encoder.finish().unwrap()
  }
}
