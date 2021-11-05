use dprint_cli_core::checksums::verify_sha256_checksum;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::PluginInfo;
use dprint_core::types::ErrBox;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::str;

use crate::environment::Environment;
use crate::utils::extract_zip;
use crate::utils::fetch_file_or_url_bytes;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::PathSource;

use super::super::SetupPluginResult;

pub fn get_file_path_from_plugin_info(plugin_info: &PluginInfo, environment: &impl Environment) -> PathBuf {
  let dir_path = get_plugin_dir_path(&plugin_info.name, &plugin_info.version, environment);
  get_plugin_executable_file_path(&dir_path, &plugin_info.name)
}

fn get_plugin_dir_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  let cache_dir_path = environment.get_cache_dir();
  cache_dir_path.join("plugins").join(&name).join(&version)
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
pub fn setup_process_plugin(url_or_file_path: &PathSource, plugin_file_bytes: &[u8], environment: &impl Environment) -> Result<SetupPluginResult, ErrBox> {
  let plugin_zip_bytes = get_plugin_zip_bytes(url_or_file_path, plugin_file_bytes, environment)?;
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_zip_bytes.name, &plugin_zip_bytes.version, environment);

  let result = setup_inner(&plugin_cache_dir_path, plugin_zip_bytes.name, &plugin_zip_bytes.zip_bytes, environment);

  return match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_verbose!(environment, "Failed setting up process plugin. {}", err);
      // failed, so delete the dir if it exists
      let _ignore = environment.remove_dir_all(&plugin_cache_dir_path);
      Err(err)
    }
  };

  fn setup_inner<TEnvironment: Environment>(
    plugin_cache_dir_path: &Path,
    plugin_name: String,
    zip_bytes: &[u8],
    environment: &TEnvironment,
  ) -> Result<SetupPluginResult, ErrBox> {
    if environment.path_exists(plugin_cache_dir_path) {
      environment.remove_dir_all(plugin_cache_dir_path)?;
    }

    extract_zip(&format!("Extracting zip for {}", plugin_name), &zip_bytes, &plugin_cache_dir_path, environment)?;

    let plugin_executable_file_path = get_plugin_executable_file_path(plugin_cache_dir_path, &plugin_name);
    if !environment.path_exists(&plugin_executable_file_path) {
      return err!(
        "Plugin zip file did not contain required executable at: {}",
        plugin_executable_file_path.display()
      );
    }

    let executable_path = super::get_test_safe_executable_path(plugin_executable_file_path.clone(), environment);
    let mut communicator = ProcessPluginCommunicator::new_with_init(&executable_path, {
      let environment = environment.clone();
      move |error_message| {
        environment.log_stderr_with_context(&error_message, &plugin_name);
      }
    })?;
    let plugin_info = communicator.get_plugin_info()?;

    Ok(SetupPluginResult {
      plugin_info,
      file_path: plugin_executable_file_path,
    })
  }
}

pub fn cleanup_process_plugin(plugin_info: &PluginInfo, environment: &impl Environment) -> Result<(), ErrBox> {
  let plugin_cache_dir_path = get_plugin_dir_path(&plugin_info.name, &plugin_info.version, environment);
  environment.remove_dir_all(&plugin_cache_dir_path)?;
  Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessPluginFile {
  schema_version: u32,
  name: String,
  version: String,
  #[serde(rename = "linux-x86_64")]
  linux: Option<ProcessPluginPath>,
  #[serde(rename = "mac-x86_64")]
  mac: Option<ProcessPluginPath>,
  #[serde(rename = "windows-x86_64")]
  windows: Option<ProcessPluginPath>,
}

#[derive(Serialize, Deserialize)]
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

fn get_plugin_zip_bytes<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<ProcessPluginZipBytes, ErrBox> {
  let plugin_file = deserialize_file(&plugin_file_bytes)?;
  let plugin_path = get_os_path(&plugin_file)?;
  let plugin_zip_path = resolve_url_or_file_path_to_path_source(&plugin_path.reference, &url_or_file_path.parent(), environment)?;
  let plugin_zip_bytes = fetch_file_or_url_bytes(&plugin_zip_path, environment)?;
  verify_sha256_checksum(&plugin_zip_bytes, &plugin_path.checksum)?;

  Ok(ProcessPluginZipBytes {
    name: plugin_file.name,
    version: plugin_file.version,
    zip_bytes: plugin_zip_bytes,
  })
}

fn deserialize_file(bytes: &[u8]) -> Result<ProcessPluginFile, ErrBox> {
  // todo: don't use serde because this should fail with a nice error message if the schema version is not equal
  let plugin_file: ProcessPluginFile = match serde_json::from_slice(&bytes) {
    Ok(plugin_file) => plugin_file,
    Err(err) => return err!("Error deserializing plugin file: {}", err.to_string()),
  };

  if plugin_file.schema_version != 1 {
    return err!(
      "Expected schema version 1, but found {}. This may indicate you need to upgrade your CLI version to use this plugin.",
      plugin_file.schema_version
    );
  }

  Ok(plugin_file)
}

fn get_os_path<'a>(plugin_file: &'a ProcessPluginFile) -> Result<&'a ProcessPluginPath, ErrBox> {
  // todo: how to throw a nice compile error here for an unsupported OS?
  #[cfg(target_os = "linux")]
  return get_plugin_path(&plugin_file.linux);

  #[cfg(target_os = "macos")]
  return get_plugin_path(&plugin_file.mac);

  #[cfg(target_os = "windows")]
  return get_plugin_path(&plugin_file.windows);
}

fn get_plugin_path<'a>(plugin_path: &'a Option<ProcessPluginPath>) -> Result<&'a ProcessPluginPath, ErrBox> {
  if let Some(path) = &plugin_path {
    Ok(path)
  } else {
    return err!("Unsupported operating system.");
  }
}
