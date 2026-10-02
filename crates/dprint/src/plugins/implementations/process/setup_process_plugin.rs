use anyhow::Result;
use anyhow::bail;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::process::ProcessPluginLaunchInfo;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::str;

use super::deno::DenoPermissions;
use super::deno::build_deno_pre_args;
use super::deno::default_deno_permissions;
use super::deno::get_allow_scripts;
use super::deno::resolve_deno_executable;
use crate::environment::Environment;
use crate::plugins::implementations::SetupPluginResult;
use crate::plugins::npm_resolution::extract_tarball_replacing;
use crate::utils::PathSource;
use crate::utils::extract_zip;
use crate::utils::fetch_file_or_url_bytes;
use crate::utils::fs::get_atomic_path;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;

/// The entrypoint every deno plugin's archive must contain.
const DENO_PLUGIN_ENTRYPOINT: &str = "main.ts";

fn get_plugin_executable_file_name(plugin_name: &str) -> String {
  if cfg!(target_os = "windows") {
    format!("{}.exe", plugin_name)
  } else {
    plugin_name.to_string()
  }
}

/// Takes a url or file path and extracts the plugin into `dest_dir_path`.
/// Returns the executable file path once complete.
/// If `pre_resolved_tarball` is provided (npm-installed process plugins), the
/// full per-platform tarball is extracted into the destination directory so
/// the executable can sit alongside any sibling files it ships. Otherwise
/// the reference inside `plugin_file_bytes` is fetched as a zip and
/// extracted. Both paths stage the extract in a sibling temp dir and rename
/// into place so a crash mid-extract can't leave a half-populated cache.
pub async fn setup_process_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  pre_resolved_tarball: Option<crate::plugins::npm_resolution::PreResolvedProcessPluginTarball>,
  dest_dir_path: &Path,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  if let Some(tarball) = pre_resolved_tarball {
    let result = setup_from_tarball(
      dest_dir_path,
      tarball.name,
      tarball.version,
      tarball.tarball_bytes,
      &tarball.executable_sub_path,
      environment,
    )
    .await;
    return match result {
      Ok(result) => Ok(result),
      Err(err) => {
        log_debug!(environment, "Failed setting up process plugin. {:#}", err);
        environment.try_remove_dir_all(dest_dir_path);
        Err(err)
      }
    };
  }

  let plugin_zip_bytes = get_plugin_zip_bytes(url_or_file_path, plugin_file_bytes, environment).await?;

  let result = setup_from_zip(
    dest_dir_path,
    plugin_zip_bytes.name,
    plugin_zip_bytes.version,
    &plugin_zip_bytes.zip_bytes,
    environment,
  )
  .await;

  match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_debug!(environment, "Failed setting up process plugin. {:#}", err);
      // failed, so delete the dir if it exists
      environment.try_remove_dir_all(dest_dir_path);
      Err(err)
    }
  }
}

/// Sets up a deno plugin from its manifest json bytes. Like a native process
/// plugin it extracts a zip into `dest_dir_path`, but the artifact is a
/// `main.ts` entrypoint run through the `deno` executable rather than a binary.
pub async fn setup_deno_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  dest_dir_path: &Path,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let deno_file = deserialize_deno_file(plugin_file_bytes)?;
  let result = setup_deno_inner(url_or_file_path, dest_dir_path, &deno_file, environment).await;

  match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_debug!(environment, "Failed setting up deno plugin. {:#}", err);
      environment.try_remove_dir_all(dest_dir_path);
      Err(err)
    }
  }
}

async fn setup_deno_inner<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  dest_dir_path: &Path,
  deno_file: &DenoPluginFile,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  // download and verify the archive
  let archive_path = resolve_url_or_file_path_to_path_source(&deno_file.archive.reference, &url_or_file_path.parent(), environment)?;
  let archive_bytes = fetch_file_or_url_bytes(&archive_path, environment).await?;
  if let Err(err) = verify_sha256_checksum(&archive_bytes, &deno_file.archive.checksum) {
    bail!(
      concat!(
        "Invalid checksum found within deno plugin's manifest file for '{}'. This is likely a ",
        "bug in the deno plugin. Please report it.\n\n{:#}",
      ),
      deno_file.archive.reference,
      err,
    );
  }

  extract_zip_into_dest(dest_dir_path, &deno_file.name, &archive_bytes, DENO_PLUGIN_ENTRYPOINT, environment)?;
  let main_ts_path = dest_dir_path.join(DENO_PLUGIN_ENTRYPOINT);
  let deno_exe = resolve_deno_executable(environment)?;

  // run `deno install` when the manifest opts into npm lifecycle scripts
  if let Some(permissions) = &deno_file.permissions
    && let Some(scripts) = get_allow_scripts(permissions)
  {
    let allow_scripts_arg = format!("--allow-scripts={}", scripts.join(","));
    log_stderr_info!(environment, "Installing dependencies for {}", deno_file.name);
    let status = environment.run_command_get_status(vec![deno_exe.as_os_str().to_owned(), "install".into(), allow_scripts_arg.into()])?;
    if status != Some(0) {
      bail!("Failed to run 'deno install' for plugin {}. Exit code: {:?}", deno_file.name, status);
    }
  }

  // run with --init to get the plugin info. Use the manifest's permissions
  // since some plugins need them at import time (e.g. prettier needs --allow-sys)
  let permissions = deno_file.permissions.clone().unwrap_or_else(default_deno_permissions);
  let launch_info = ProcessPluginLaunchInfo {
    executable: deno_exe,
    pre_args: build_deno_pre_args(&permissions, dest_dir_path, &main_ts_path),
  };
  let plugin_name = deno_file.name.clone();
  let communicator = ProcessPluginCommunicator::new_with_init_launch_info(&launch_info, {
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
    file_path: main_ts_path,
    executable_sub_path: Some(DENO_PLUGIN_ENTRYPOINT.to_string()),
    deno_permissions: Some(permissions),
  })
}

async fn setup_from_zip<TEnvironment: Environment>(
  plugin_cache_dir_path: &Path,
  plugin_name: String,
  plugin_version: String,
  zip_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let executable_sub_path = get_plugin_executable_file_name(&plugin_name);
  extract_zip_into_dest(plugin_cache_dir_path, &plugin_name, zip_bytes, &executable_sub_path, environment)?;

  let plugin_executable_file_path = plugin_cache_dir_path.join(&executable_sub_path);
  start_communicator_and_collect_info(plugin_executable_file_path, executable_sub_path, plugin_version, plugin_name, environment).await
}

/// Extracts a plugin's zip into `plugin_cache_dir_path`, staging it in a
/// sibling temp dir and renaming into place so a crash mid-extract can't leave
/// the destination half-populated for a future run to mistake as "already set
/// up". Verifies `required_sub_path` is present before committing. The caller's
/// fs_lock prevents a competing setup against the same source.
fn extract_zip_into_dest<TEnvironment: Environment>(
  plugin_cache_dir_path: &Path,
  plugin_name: &str,
  zip_bytes: &[u8],
  required_sub_path: &str,
  environment: &TEnvironment,
) -> Result<()> {
  let temp_dir = get_atomic_path(environment, plugin_cache_dir_path);
  environment.mk_dir_all(&temp_dir)?;
  if let Err(err) = extract_zip(&format!("Extracting zip for {}", plugin_name), zip_bytes, &temp_dir, environment) {
    environment.try_remove_dir_all(&temp_dir);
    return Err(err);
  }
  let temp_required_path = temp_dir.join(required_sub_path);
  if !environment.path_exists(&temp_required_path) {
    environment.try_remove_dir_all(&temp_dir);
    bail!("Plugin zip file did not contain required file at: {}", temp_required_path.display());
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
  Ok(())
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
  plugin_version: String,
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
  start_communicator_and_collect_info(executable_path, executable_sub_path.to_string(), plugin_version, plugin_name, environment).await
}

async fn start_communicator_and_collect_info<TEnvironment: Environment>(
  plugin_executable_file_path: PathBuf,
  executable_sub_path: String,
  plugin_version: String,
  plugin_name: String,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let executable_path = super::get_test_safe_executable_path(&plugin_version, plugin_executable_file_path.clone(), environment);
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
    executable_sub_path: Some(executable_sub_path),
    deno_permissions: None,
  })
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
  #[serde(rename = "linux-powerpc64")]
  pub linux_powerpc64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-powerpc64-musl")]
  pub linux_powerpc64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "android-x86_64")]
  pub android_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "android-aarch64")]
  pub android_aarch64: Option<ProcessPluginPath>,
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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DenoPluginFile {
  #[allow(dead_code)]
  schema_version: u32,
  #[allow(dead_code)]
  kind: String,
  name: String,
  #[allow(dead_code)]
  version: String,
  archive: ProcessPluginPath,
  permissions: Option<DenoPermissions>,
}

fn deserialize_deno_file(bytes: &[u8]) -> Result<DenoPluginFile> {
  let plugin_file: Value = match serde_json::from_slice(bytes) {
    Ok(plugin_file) => plugin_file,
    Err(err) => bail!(
      "Error deserializing deno plugin file: {}\n\nThis might mean you're using an old version of dprint.",
      err
    ),
  };

  verify_plugin_file(&plugin_file)?;

  Ok(serde_json::value::from_value(plugin_file)?)
}

/// Peeks at the `kind` field of a plugin manifest's json bytes. Deno and native
/// process plugins share the same `.json` file extension, so this is the only
/// way to tell them apart before parsing into the kind-specific manifest type.
pub fn peek_plugin_kind(bytes: &[u8]) -> Option<String> {
  let value: Value = serde_json::from_slice(bytes).ok()?;
  value.as_object()?.get("kind")?.as_str().map(|s| s.to_string())
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
  if schema_version != Some(2) && schema_version != Some(3) {
    bail!(
      "Expected schema version 2 or 3, but found {}. This may indicate you need to upgrade your CLI version or plugin.",
      schema_version.map(|v| v.to_string()).unwrap_or_else(|| "no property".to_string())
    );
  }

  let kind = plugin_file.as_object().and_then(|o| o.get("kind")).and_then(|v| v.as_str());

  if let Some(kind) = kind
    && kind != "process"
    && kind != "deno"
  {
    bail!("Unsupported plugin kind: {kind}\nOnly process and deno plugins are supported by this version of dprint. Please upgrade your CLI.");
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
      "powerpc64" => plugin_file.linux_powerpc64.as_ref(),
      _ => None,
    },
    "linux-musl" => match arch.as_str() {
      "x86_64" => plugin_file.linux_x86_64_musl.as_ref(),
      "aarch64" => plugin_file.linux_aarch64_musl.as_ref().or(plugin_file.linux_x86_64_musl.as_ref()),
      "riscv64" => plugin_file.linux_riscv64_musl.as_ref(),
      "loongarch64" => plugin_file.linux_loongarch64_musl.as_ref(),
      "powerpc64" => plugin_file.linux_powerpc64_musl.as_ref(),
      _ => None,
    },
    // android (Termux) uses bionic libc, so it's neither linux nor linux-musl
    "android" => match arch.as_str() {
      "x86_64" => plugin_file.android_x86_64.as_ref(),
      "aarch64" => plugin_file.android_aarch64.as_ref().or(plugin_file.android_x86_64.as_ref()),
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
  fn ensure_valid_kinds_allowed() {
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2, "kind": "process" }"#.as_bytes()).unwrap()).is_ok());
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2 }"#.as_bytes()).unwrap()).is_ok());
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 3, "kind": "deno" }"#.as_bytes()).unwrap()).is_ok());
    assert!(verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 3, "kind": "process" }"#.as_bytes()).unwrap()).is_ok());
    assert_eq!(
      verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 2, "kind": "other" }"#.as_bytes()).unwrap())
        .err()
        .unwrap()
        .to_string(),
      "Unsupported plugin kind: other\nOnly process and deno plugins are supported by this version of dprint. Please upgrade your CLI.",
    );
    assert!(
      verify_plugin_file(&serde_json::from_slice(r#"{ "schemaVersion": 4 }"#.as_bytes()).unwrap())
        .err()
        .unwrap()
        .to_string()
        .contains("Expected schema version 2 or 3")
    );
  }

  #[test]
  fn ensure_peek_plugin_kind() {
    assert_eq!(
      peek_plugin_kind(r#"{ "schemaVersion": 3, "kind": "deno" }"#.as_bytes()).as_deref(),
      Some("deno")
    );
    assert_eq!(peek_plugin_kind(r#"{ "schemaVersion": 2 }"#.as_bytes()), None);
    assert_eq!(peek_plugin_kind(b"not json"), None);
  }
}
