use anyhow::Result;
use anyhow::bail;
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::str;

use crate::environment::Environment;
use crate::plugins::implementations::SetupPluginResult;
use crate::plugins::npm_resolution::extract_tarball_replacing;
use crate::utils::PathSource;
use crate::utils::extract_zip;
use crate::utils::fetch_file_or_url_bytes;
use crate::utils::fs::get_atomic_path;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;

pub fn get_file_path_from_plugin_info(plugin_info: &PluginInfo, environment: &impl Environment) -> PathBuf {
  get_file_path_from_name_and_version(&plugin_info.name, &plugin_info.version, environment)
}

pub fn get_file_path_from_name_and_version(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  let dir_path = get_plugin_dir_path(name, version, environment);
  get_plugin_executable_file_path(&dir_path, name)
}

fn get_plugin_dir_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  let cache_dir_path = environment.get_cache_dir();
  cache_dir_path.join("plugins").join(name).join(version).join(environment.cpu_arch())
}

fn get_plugin_executable_file_path(dir_path: &Path, plugin_name: &str) -> PathBuf {
  dir_path.join(if cfg!(target_os = "windows") {
    format!("{}.exe", plugin_name)
  } else {
    plugin_name.to_string()
  })
}

/// Takes a url or file path and extracts the plugin to a cache folder.
/// Returns the executable file path once complete.
/// If `pre_resolved_tarball` is provided (npm-installed process plugins), the
/// full per-platform tarball is extracted into the plugin cache directory so
/// the executable can sit alongside any sibling files it ships. Otherwise
/// the reference inside `plugin_file_bytes` is fetched as a zip and
/// extracted. Both paths stage the extract in a sibling temp dir and rename
/// into place so a crash mid-extract can't leave a half-populated cache.
pub async fn setup_process_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  pre_resolved_tarball: Option<crate::plugins::npm_resolution::PreResolvedProcessPluginTarball>,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  if let Some(tarball) = pre_resolved_tarball {
    let plugin_cache_dir_path = get_plugin_dir_path(&tarball.name, &tarball.version, environment);
    let result = setup_from_tarball(
      &plugin_cache_dir_path,
      tarball.name,
      tarball.tarball_bytes,
      &tarball.executable_sub_path,
      environment,
    )
    .await;
    return match result {
      Ok(result) => Ok(result),
      Err(err) => {
        log_debug!(environment, "Failed setting up process plugin. {:#}", err);
        environment.try_remove_dir_all(&plugin_cache_dir_path);
        Err(err)
      }
    };
  }

  let plugin_zip_bytes = get_plugin_zip_bytes(url_or_file_path, plugin_file_bytes, environment).await?;
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_zip_bytes.name, &plugin_zip_bytes.version, environment);

  let result = setup_from_zip(&plugin_cache_dir_path, plugin_zip_bytes.name, &plugin_zip_bytes.zip_bytes, environment).await;

  match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_debug!(environment, "Failed setting up process plugin. {:#}", err);
      // failed, so delete the dir if it exists
      environment.try_remove_dir_all(&plugin_cache_dir_path);
      Err(err)
    }
  }
}

async fn setup_from_zip<TEnvironment: Environment>(
  plugin_cache_dir_path: &Path,
  plugin_name: String,
  zip_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  // stage the extract in a sibling temp dir so a crash mid-extract can't
  // leave the destination half-populated for a future run to mistake as
  // "already set up". Caller's fs_lock prevents a competing setup against
  // the same source.
  let temp_dir = get_atomic_path(environment, plugin_cache_dir_path);
  environment.mk_dir_all(&temp_dir)?;
  if let Err(err) = extract_zip(&format!("Extracting zip for {}", plugin_name), zip_bytes, &temp_dir, environment) {
    environment.try_remove_dir_all(&temp_dir);
    return Err(err);
  }
  let temp_executable = get_plugin_executable_file_path(&temp_dir, &plugin_name);
  if !environment.path_exists(&temp_executable) {
    environment.try_remove_dir_all(&temp_dir);
    bail!("Plugin zip file did not contain required executable at: {}", temp_executable.display(),);
  }
  // remove any existing directory before moving the staged extract into place.
  // surface a removal failure directly — otherwise the rename below fails with
  // a confusing "directory not empty" error that hides the real cause.
  if let Err(err) = environment.remove_dir_all(plugin_cache_dir_path) {
    environment.try_remove_dir_all(&temp_dir);
    return Err(err.into());
  }
  if let Err(err) = environment.rename(&temp_dir, plugin_cache_dir_path) {
    environment.try_remove_dir_all(&temp_dir);
    return Err(err.into());
  }

  let plugin_executable_file_path = get_plugin_executable_file_path(plugin_cache_dir_path, &plugin_name);
  start_communicator_and_collect_info(plugin_executable_file_path, plugin_name, environment).await
}

/// Extracts a per-platform npm tarball into the plugin cache directory. The
/// tarball is fully unpacked (wrapper directory stripped, file modes
/// preserved) so the executable can reference siblings that ship in the
/// same package. `executable_sub_path` is the binary's path inside the
/// tarball's top-level wrapper — i.e. the same string the plugin.json
/// reference carries after the version.
async fn setup_from_tarball<TEnvironment: Environment>(
  plugin_cache_dir_path: &Path,
  plugin_name: String,
  tarball_bytes: Vec<u8>,
  executable_sub_path: &str,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let executable_path = plugin_cache_dir_path.join(executable_sub_path);
  let extract_env = environment.clone();
  let extract_dest = plugin_cache_dir_path.to_path_buf();
  // tarball decompression + file I/O blocks; keep it off the runtime thread.
  dprint_core::async_runtime::spawn_blocking(move || extract_tarball_replacing(&tarball_bytes, &extract_dest, &extract_env)).await??;

  if !environment.path_exists(&executable_path) {
    bail!(
      "Tarball for {} did not contain the executable at the path given by the plugin.json reference ({}).",
      plugin_name,
      executable_sub_path,
    );
  }
  start_communicator_and_collect_info(executable_path, plugin_name, environment).await
}

async fn start_communicator_and_collect_info<TEnvironment: Environment>(
  plugin_executable_file_path: PathBuf,
  plugin_name: String,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let executable_path = super::get_test_safe_executable_path(plugin_executable_file_path.clone(), environment);
  let communicator = ProcessPluginCommunicator::new_with_init(&executable_path, {
    let environment = environment.clone();
    move |error_message| {
      // consider messages from process plugins as warnings
      if environment.log_level().is_warn() {
        environment.log_stderr_with_context(&error_message, &plugin_name);
      }
    }
  })
  .await?;
  let plugin_info = communicator.plugin_info().await?;
  communicator.shutdown().await;

  Ok(SetupPluginResult {
    plugin_info,
    file_path: plugin_executable_file_path,
  })
}

pub fn cleanup_process_plugin(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<()> {
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_info.name, &plugin_info.version, environment);
  environment.remove_dir_all(plugin_cache_dir_path)?;
  Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginFile {
  pub schema_version: u32,
  pub name: String,
  pub version: String,
  #[serde(rename = "linux-x86_64")]
  pub linux_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-x86_64-musl")]
  pub linux_x86_64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "linux-aarch64")]
  pub linux_aarch64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-aarch64-musl")]
  pub linux_aarch64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "linux-riscv64")]
  pub linux_riscv64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-riscv64-musl")]
  pub linux_riscv64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "linux-loongarch64")]
  pub linux_loongarch64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-loongarch64-musl")]
  pub linux_loongarch64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "darwin-x86_64")]
  pub darwin_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "darwin-aarch64")]
  pub darwin_aarch64: Option<ProcessPluginPath>,
  #[serde(rename = "windows-x86_64")]
  pub windows_x64_64: Option<ProcessPluginPath>,
  #[serde(rename = "windows-aarch64")]
  pub windows_aarch64: Option<ProcessPluginPath>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPluginPath {
  pub reference: String,
  pub checksum: String,
}

struct ProcessPluginZipBytes {
  name: String,
  version: String,
  zip_bytes: Vec<u8>,
}

async fn get_plugin_zip_bytes<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<ProcessPluginZipBytes> {
  let plugin_file = parse_process_plugin_file(plugin_file_bytes)?;
  let plugin_path = get_os_path(&plugin_file, environment)?;
  let plugin_zip_path = resolve_url_or_file_path_to_path_source(&plugin_path.reference, &url_or_file_path.parent(), environment)?;
  let plugin_zip_bytes = fetch_file_or_url_bytes(&plugin_zip_path, environment).await?;
  if let Err(err) = verify_sha256_checksum(&plugin_zip_bytes, &plugin_path.checksum) {
    bail!(
      concat!(
        "Invalid checksum found within process plugin's manifest file for '{}'. This is likely a ",
        "bug in the process plugin. Please report it.\n\n{:#}",
      ),
      plugin_path.reference,
      err,
    )
  }

  Ok(ProcessPluginZipBytes {
    name: plugin_file.name,
    version: plugin_file.version,
    zip_bytes: plugin_zip_bytes,
  })
}

pub fn parse_process_plugin_file(bytes: &[u8]) -> Result<ProcessPluginFile> {
  let plugin_file: Value = match serde_json::from_slice(bytes) {
    Ok(plugin_file) => plugin_file,
    Err(err) => bail!(
      "Error deserializing plugin file: {}\n\nThis might mean you're using an old version of dprint.",
      err
    ),
  };

  verify_plugin_file(&plugin_file)?;

  Ok(serde_json::value::from_value(plugin_file)?)
}

fn verify_plugin_file(plugin_file: &Value) -> Result<()> {
  let schema_version = plugin_file.as_object().and_then(|o| o.get("schemaVersion")).and_then(|v| v.as_u64());
  if schema_version != Some(2) {
    bail!(
      "Expected schema version 2, but found {}. This may indicate you need to upgrade your CLI version or plugin.",
      schema_version.map(|v| v.to_string()).unwrap_or_else(|| "no property".to_string())
    );
  }

  let kind = plugin_file.as_object().and_then(|o| o.get("kind")).and_then(|v| v.as_str());

  if let Some(kind) = kind
    && kind != "process"
  {
    bail!("Unsupported plugin kind: {kind}\nOnly process plugins are supported by this version of dprint. Please upgrade your CLI.");
  }

  Ok(())
}

pub fn get_os_path<'a>(plugin_file: &'a ProcessPluginFile, environment: &impl Environment) -> Result<&'a ProcessPluginPath> {
  let arch = environment.cpu_arch();
  let os = environment.os();
  let path = match os.as_str() {
    "linux" => match arch.as_str() {
      "x86_64" => plugin_file.linux_x86_64.as_ref(),
      "aarch64" => plugin_file.linux_aarch64.as_ref().or(plugin_file.linux_x86_64.as_ref()),
      "riscv64" => plugin_file.linux_riscv64.as_ref(),
      "loongarch64" => plugin_file.linux_loongarch64.as_ref(),
      _ => None,
    },
    "linux-musl" => match arch.as_str() {
      "x86_64" => plugin_file.linux_x86_64_musl.as_ref(),
      "aarch64" => plugin_file.linux_aarch64_musl.as_ref().or(plugin_file.linux_x86_64_musl.as_ref()),
      "riscv64" => plugin_file.linux_riscv64_musl.as_ref(),
      "loongarch64" => plugin_file.linux_loongarch64_musl.as_ref(),
      _ => None,
    },
    "macos" => match arch.as_str() {
      "x86_64" => plugin_file.darwin_x86_64.as_ref(),
      "aarch64" => plugin_file.darwin_aarch64.as_ref().or(plugin_file.darwin_x86_64.as_ref()),
      _ => None,
    },
    "windows" => match arch.as_str() {
      "x86_64" => plugin_file.windows_x64_64.as_ref(),
      "aarch64" => plugin_file.windows_aarch64.as_ref().or(plugin_file.windows_x64_64.as_ref()),
      _ => None,
    },
    _ => bail!("Unsupported operating system: {}", os),
  };

  match path {
    Some(path) => Ok(path),
    None => {
      log_debug!(environment, "Plugin File -- {:#?}", plugin_file);
      bail!("Unsupported CPU architecture: {} ({})", arch, os)
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn ensure_only_process_kind_allowed() {
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2, "kind": "process" }"#.as_bytes()).unwrap()).is_ok(),);
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2 }"#.as_bytes()).unwrap()).is_ok(),);
    assert_eq!(
      verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2, "kind": "other" }"#.as_bytes()).unwrap())
        .err()
        .unwrap()
        .to_string(),
      "Unsupported plugin kind: other\nOnly process plugins are supported by this version of dprint. Please upgrade your CLI.",
    );
  }
}
