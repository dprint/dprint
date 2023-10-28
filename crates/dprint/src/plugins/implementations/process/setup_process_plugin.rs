use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::PluginInfo;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::str;

use crate::environment::Environment;
use crate::plugins::implementations::SetupPluginResult;
use crate::utils::extract_zip;
use crate::utils::fetch_file_or_url_bytes;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;
use crate::utils::PathSource;

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
/// Returns the executable file path once complete
pub async fn setup_process_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let plugin_zip_bytes = get_plugin_zip_bytes(url_or_file_path, plugin_file_bytes, environment).await?;
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_zip_bytes.name, &plugin_zip_bytes.version, environment);

  let result = setup_inner(&plugin_cache_dir_path, plugin_zip_bytes.name, &plugin_zip_bytes.zip_bytes, environment).await;

  return match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_debug!(environment, "Failed setting up process plugin. {:#}", err);
      // failed, so delete the dir if it exists
      let _ignore = environment.remove_dir_all(&plugin_cache_dir_path);
      Err(err)
    }
  };

  async fn setup_inner<TEnvironment: Environment>(
    plugin_cache_dir_path: &Path,
    plugin_name: String,
    zip_bytes: &[u8],
    environment: &TEnvironment,
  ) -> Result<SetupPluginResult> {
    let _ = environment.remove_dir_all(plugin_cache_dir_path);
    environment.mk_dir_all(plugin_cache_dir_path)?;
    let plugin_executable_file_path = get_plugin_executable_file_path(plugin_cache_dir_path, &plugin_name);

    extract_zip(&format!("Extracting zip for {}", plugin_name), zip_bytes, plugin_cache_dir_path, environment)?;

    if !environment.path_exists(&plugin_executable_file_path) {
      bail!(
        "Plugin zip file did not contain required executable at: {}",
        plugin_executable_file_path.display()
      );
    }

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
}

pub fn cleanup_process_plugin(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<()> {
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_info.name, &plugin_info.version, environment);
  environment.remove_dir_all(plugin_cache_dir_path)?;
  Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ProcessPluginFile {
  schema_version: u32,
  name: String,
  version: String,
  #[serde(rename = "linux-x86_64")]
  linux_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-x86_64-musl")]
  linux_x86_64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "linux-aarch64")]
  linux_aarch64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-aarch64-musl")]
  linux_aarch64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "darwin-x86_64")]
  darwin_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "darwin-aarch64")]
  darwin_aarch64: Option<ProcessPluginPath>,
  #[serde(rename = "windows-x86_64")]
  windows: Option<ProcessPluginPath>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ProcessPluginPath {
  reference: String,
  checksum: String,
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
  let plugin_file = deserialize_file(plugin_file_bytes)?;
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

fn deserialize_file(bytes: &[u8]) -> Result<ProcessPluginFile> {
  let plugin_file: Value = match serde_json::from_slice(bytes) {
    Ok(plugin_file) => plugin_file,
    Err(err) => bail!(
      "Error deserializing plugin file: {}\n\nThis might mean you're using an old version of dprint.",
      err.to_string()
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

  if let Some(kind) = kind {
    if kind != "process" {
      bail!("Unsupported plugin kind: {kind}\nOnly process plugins are supported by this version of dprint. Please upgrade your CLI.");
    }
  }

  Ok(())
}

fn get_os_path<'a>(plugin_file: &'a ProcessPluginFile, environment: &impl Environment) -> Result<&'a ProcessPluginPath> {
  let arch = environment.cpu_arch();
  let os = environment.os();
  let path = match os.as_str() {
    "linux" => match arch.as_str() {
      "x86_64" => plugin_file.linux_x86_64.as_ref(),
      "aarch64" => plugin_file.linux_aarch64.as_ref().or(plugin_file.linux_x86_64.as_ref()),
      _ => None,
    },
    "linux-musl" => match arch.as_str() {
      "x86_64" => plugin_file.linux_x86_64_musl.as_ref(),
      "aarch64" => plugin_file.linux_aarch64_musl.as_ref().or(plugin_file.linux_x86_64_musl.as_ref()),
      _ => None,
    },
    "macos" => match arch.as_str() {
      "x86_64" => plugin_file.darwin_x86_64.as_ref(),
      "aarch64" => plugin_file.darwin_aarch64.as_ref().or(plugin_file.darwin_x86_64.as_ref()),
      _ => None,
    },
    "windows" => match arch.as_str() {
      "x86_64" => plugin_file.windows.as_ref(),
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
