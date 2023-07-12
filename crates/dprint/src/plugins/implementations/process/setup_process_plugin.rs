use anyhow::bail;
use anyhow::Result;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::process::ProcessPluginExecutableInfo;
use dprint_core::plugins::NoopHost;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::implementations::SetupPluginResult;
use crate::utils::extract_zip;
use crate::utils::fetch_file_or_url_bytes;
use crate::utils::resolve_url_or_file_path_to_path_source;
use crate::utils::verify_sha256_checksum;
use crate::utils::PathSource;

use super::resolve_node_executable;
use super::resolve_npm_executable;

pub fn get_exec_plugin_file_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  get_exec_plugin_dir_path(name, version, environment).join(get_exec_plugin_file_name(name))
}

pub fn get_exec_plugin_dir_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  let cache_dir_path = environment.get_cache_dir();
  cache_dir_path.join("plugins").join(name).join(version).join(environment.cpu_arch())
}

pub fn get_node_plugin_file_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  get_node_plugin_dir_path(name, version, environment).join("main.mjs")
}

pub fn get_node_plugin_dir_path(name: &str, version: &str, environment: &impl Environment) -> PathBuf {
  let cache_dir_path = environment.get_cache_dir();
  cache_dir_path.join("plugins").join(name).join(version)
}

fn get_exec_plugin_file_name(plugin_name: &str) -> String {
  // this should be the name of the plugin so it shows up nicely in task manager
  if cfg!(target_os = "windows") {
    format!("{}.exe", plugin_name)
  } else {
    plugin_name.to_string()
  }
}

/// Takes a url or file path and extracts the plugin to a cache folder.
/// Returns the executable file path once complete
pub async fn setup_process_plugin<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_file_bytes: &[u8],
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let plugin_file = deserialize_file(plugin_file_bytes)?;
  let plugin_cache_dir_path = match &plugin_file {
    ProcessPluginFile::Exec(_) => get_exec_plugin_dir_path(plugin_file.name(), plugin_file.version(), environment),
    ProcessPluginFile::Node(_) => get_node_plugin_dir_path(plugin_file.name(), plugin_file.version(), environment),
  };

  let _ = environment.remove_dir_all(&plugin_cache_dir_path);
  environment.mk_dir_all(&plugin_cache_dir_path)?;

  let result = match plugin_file {
    ProcessPluginFile::Exec(plugin_file) => setup_exec(url_or_file_path, &plugin_cache_dir_path, plugin_file, environment).await,
    ProcessPluginFile::Node(plugin_file) => setup_node(url_or_file_path, &plugin_cache_dir_path, plugin_file, environment).await,
  };

  match result {
    Ok(result) => Ok(result),
    Err(err) => {
      log_verbose!(environment, "Failed setting up process plugin. {:#}", err);
      // failed, so delete the dir if it exists
      let _ignore = environment.remove_dir_all(&plugin_cache_dir_path);
      Err(err)
    }
  }
}

async fn setup_exec<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_cache_dir_path: &Path,
  plugin_file: ExecProcessPluginFile,
  environment: &TEnvironment,
) -> Result<SetupPluginResult> {
  let plugin_path = get_os_path(&plugin_file, environment)?;
  let zip_bytes = get_plugin_zip_bytes(url_or_file_path, plugin_path, environment)?;
  let plugin_executable_file_path = plugin_cache_dir_path.join(get_exec_plugin_file_name(&plugin_file.name));

  extract_zip(
    &format!("Extracting zip for {}", plugin_file.name),
    &zip_bytes,
    plugin_cache_dir_path,
    environment,
  )?;

  if !environment.path_exists(&plugin_executable_file_path) {
    bail!(
      "Plugin zip file did not contain required executable at: {}",
      plugin_executable_file_path.display()
    );
  }

  let executable_path = super::get_test_safe_executable_path(plugin_executable_file_path.clone(), environment);
  let communicator = ProcessPluginCommunicator::new_with_init(
    &ProcessPluginExecutableInfo {
      path: executable_path,
      args: Vec::new(),
    },
    {
      let environment = environment.clone();
      move |error_message| {
        environment.log_stderr_with_context(&error_message, &plugin_file.name);
      }
    },
    // it's ok to use a no-op host here because
    // we're only getting the plugin information
    Arc::new(NoopHost),
  )
  .await?;
  let plugin_info = communicator.plugin_info().await?;
  communicator.shutdown().await;

  return Ok(SetupPluginResult::Exec { plugin_info });

  fn get_os_path<'a>(plugin_file: &'a ExecProcessPluginFile, environment: &impl Environment) -> Result<&'a ProcessPluginPath> {
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
        log_verbose!(environment, "Plugin File -- {:#?}", plugin_file);
        bail!("Unsupported CPU architecture: {} ({})", arch, os)
      }
    }
  }
}

async fn setup_node<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_cache_dir_path: &Path,
  plugin_file: NodeProcessPluginFile,
  environment: &TEnvironment,
) -> std::result::Result<SetupPluginResult, anyhow::Error> {
  let executable_path = resolve_node_executable(environment)?.clone();
  let zip_bytes = get_plugin_zip_bytes(url_or_file_path, &plugin_file.archive, environment)?;
  let plugin_mjs_file_path = plugin_cache_dir_path.join("main.mjs");

  extract_zip(
    &format!("Extracting zip for {}", plugin_file.name),
    &zip_bytes,
    plugin_cache_dir_path,
    environment,
  )?;

  if !environment.path_exists(&plugin_mjs_file_path) {
    bail!("Plugin zip file did not contain required script file at: {}", plugin_mjs_file_path.display());
  }
  if environment.path_exists(plugin_cache_dir_path.join("package.json")) {
    let npm_executable_path = resolve_npm_executable(environment)?.clone();
    log_verbose!(environment, "Running npm install.");
    let exit_code = Command::new(npm_executable_path)
      .arg("install")
      .stdout(Stdio::null())
      .current_dir(&plugin_cache_dir_path)
      .status()?;
    if !exit_code.success() {
      bail!("Failed to install npm dependencies for plugin: {}", plugin_file.name);
    }
  }

  let snapshottable = environment.path_exists(plugin_cache_dir_path.join("snapshot.mjs"));

  let communicator = ProcessPluginCommunicator::new_with_init(
    &ProcessPluginExecutableInfo {
      path: executable_path,
      args: vec![plugin_mjs_file_path.to_string_lossy().to_string()],
    },
    {
      let environment = environment.clone();
      move |error_message| {
        environment.log_stderr_with_context(&error_message, &plugin_file.name);
      }
    },
    // it's ok to use a no-op host here because
    // we're only getting the plugin information
    Arc::new(NoopHost),
  )
  .await?;
  let plugin_info = communicator.plugin_info().await?;
  communicator.shutdown().await;

  Ok(SetupPluginResult::Node { plugin_info, snapshottable })
}

fn get_plugin_zip_bytes<TEnvironment: Environment>(
  url_or_file_path: &PathSource,
  plugin_path: &ProcessPluginPath,
  environment: &TEnvironment,
) -> Result<Vec<u8>> {
  let plugin_zip_path = resolve_url_or_file_path_to_path_source(&plugin_path.reference, &url_or_file_path.parent(), environment)?;
  let plugin_zip_bytes = fetch_file_or_url_bytes(&plugin_zip_path, environment)?;
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

  Ok(plugin_zip_bytes)
}

#[allow(clippy::large_enum_variant)]
enum ProcessPluginFile {
  Exec(ExecProcessPluginFile),
  Node(NodeProcessPluginFile),
}

impl ProcessPluginFile {
  pub fn name(&self) -> &str {
    match self {
      ProcessPluginFile::Exec(file) => file.name.as_str(),
      ProcessPluginFile::Node(file) => file.name.as_str(),
    }
  }

  pub fn version(&self) -> &str {
    match self {
      ProcessPluginFile::Exec(file) => file.version.as_str(),
      ProcessPluginFile::Node(file) => file.version.as_str(),
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ExecProcessPluginFile {
  name: String,
  version: String,
  #[serde(rename = "linux-x86_64")]
  linux_x86_64: Option<ProcessPluginPath>,
  #[serde(rename = "linux-x86_64-musl")]
  linux_x86_64_musl: Option<ProcessPluginPath>,
  #[serde(rename = "linux-aarch64")]
  linux_aarch64: Option<ProcessPluginPath>,
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NodeProcessPluginFile {
  name: String,
  version: String,
  archive: ProcessPluginPath,
}

fn deserialize_file(bytes: &[u8]) -> Result<ProcessPluginFile> {
  let plugin_file: Value = match serde_json::from_slice(bytes) {
    Ok(plugin_file) => plugin_file,
    Err(err) => bail!(
      "Error deserializing plugin file: {}\n\nThis might mean you're using an old version of dprint.",
      err.to_string()
    ),
  };

  let schema_version = plugin_file.as_object().and_then(|o| o.get("schemaVersion")).and_then(|v| v.as_u64());
  if schema_version != Some(2) {
    bail!(
      "Expected schema version 2, but found {}. This may indicate you need to upgrade your CLI version or plugin.",
      schema_version.map(|v| v.to_string()).unwrap_or_else(|| "no property".to_string())
    );
  }

  let kind = plugin_file.as_object().and_then(|o| o.get("kind")).and_then(|v| v.as_str());

  match kind {
    Some("process") | None => Ok(ProcessPluginFile::Exec(serde_json::from_value::<ExecProcessPluginFile>(plugin_file)?)),
    Some("node") => Ok(ProcessPluginFile::Node(serde_json::from_value::<NodeProcessPluginFile>(plugin_file)?)),
    Some(kind) => {
      bail!(
        concat!(
          "Unsupported plugin kind: {}\n",
          "Only node and process plugins are supported by this ",
          "version of dprint. Please upgrade your CLI."
        ),
        kind
      );
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn ensure_only_node_and_process_kind_allowed() {
    assert!(deserialize_file(r#"{ "schemaVersion": 2, "kind": "process", "name": "test", "version": "1.0.0" }"#.as_bytes()).is_ok());
    assert!(deserialize_file(r#"{ "schemaVersion": 2, "name": "test", "version": "1.0.0" }"#.as_bytes()).is_ok());
    assert_eq!(
      deserialize_file(r#"{ "schemaVersion": 2, "kind": "other" }"#.as_bytes())
        .err()
        .unwrap()
        .to_string(),
      "Unsupported plugin kind: other\nOnly node and process plugins are supported by this version of dprint. Please upgrade your CLI.",
    );
  }
}
