use bytes::Bytes;
use std::str;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use crate::environment::Environment;
use crate::utils::{PathSource, fetch_file_or_url_bytes, resolve_url_or_file_path_to_path_source, verify_sha256_checksum, extract_zip};
use crate::types::ErrBox;

/// Takes a url or file path and extracts the plugin to a cache folder.
/// Returns the executable file path once complete
async fn setup_process_plugin(url_or_file_path: &PathSource, checksum: &str, environment: &impl Environment) -> Result<PathBuf, ErrBox> {
    let cache_dir_path = environment.get_cache_dir()?;
    let plugin_zip_bytes = get_plugin_zip_bytes(url_or_file_path, checksum, environment).await?;
    let plugin_cache_dir_path = cache_dir_path.join("plugins").join(&plugin_zip_bytes.name).join(&plugin_zip_bytes.version);

    let result = setup_inner(&plugin_cache_dir_path, plugin_zip_bytes.name, &plugin_zip_bytes.zip_bytes, environment);

    return match result {
        Ok(plugin_cache_dir_path) => Ok(plugin_cache_dir_path),
        Err(err) => {
            // failed, so delete the dir if it exists
            let _ignore = environment.remove_dir_all(&plugin_cache_dir_path);
            Err(err)
        }
    };

    fn setup_inner(plugin_cache_dir_path: &PathBuf, plugin_name: String, zip_bytes: &[u8], environment: &impl Environment) -> Result<PathBuf, ErrBox> {
        environment.remove_dir_all(&plugin_cache_dir_path)?;

        extract_zip(&zip_bytes, &plugin_cache_dir_path, environment)?;

        let plugin_executable_file_path = plugin_cache_dir_path.join(if cfg!(target_os="windows") {
            format!("{}.exe", plugin_name)
        } else {
            plugin_name
        });

        if !environment.path_exists(&plugin_executable_file_path) {
            return err!("Plugin zip file did not contain required executable at: {}", plugin_executable_file_path.display());
        }

        Ok(plugin_executable_file_path)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessPluginFile {
    schema_version: u32,
    name: String,
    version: String,
    #[serde(rename="linux-x86_64")]
    linux: Option<ProcessPluginPath>,
    #[serde(rename="mac-x86_64")]
    mac: Option<ProcessPluginPath>,
    #[serde(rename="windows-x86_64")]
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
    zip_bytes: Bytes,
}

async fn get_plugin_zip_bytes<TEnvironment: Environment>(url_or_file_path: &PathSource, checksum: &str, environment: &TEnvironment) -> Result<ProcessPluginZipBytes, ErrBox> {
    let plugin_file_bytes = fetch_file_or_url_bytes(url_or_file_path, environment).await?;
    verify_sha256_checksum(&plugin_file_bytes, checksum)?;
    let plugin_file = deserialize_file(&plugin_file_bytes)?;
    let plugin_path = get_os_path(&plugin_file)?;
    let plugin_zip_path = resolve_url_or_file_path_to_path_source(&plugin_path.reference, &url_or_file_path.parent())?;
    let plugin_zip_bytes = fetch_file_or_url_bytes(&plugin_zip_path, environment).await?;
    verify_sha256_checksum(&plugin_zip_bytes, &plugin_path.reference)?;

    Ok(ProcessPluginZipBytes {
        name: plugin_file.name,
        version: plugin_file.version,
        zip_bytes: plugin_zip_bytes,
    })
}

fn deserialize_file(bytes: &[u8]) -> Result<ProcessPluginFile, ErrBox> {
    let plugin_file: ProcessPluginFile = match serde_json::from_slice(&bytes) {
        Ok(plugin_file) => plugin_file,
        Err(err) => return err!("Error deserializing plugin file. {:?}", err),
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
    #[cfg(target_os="linux")]
    return get_plugin_path(&plugin_file.linux);

    #[cfg(target_os="macos")]
    return get_plugin_path(&plugin_file.mac);

    #[cfg(target_os="windows")]
    return get_plugin_path(&plugin_file.windows);
}

fn get_plugin_path<'a>(plugin_path: &'a Option<ProcessPluginPath>) -> Result<&'a ProcessPluginPath, ErrBox> {
    if let Some(path) = &plugin_path {
        Ok(path)
    } else {
        return err!("Unsupported operating system for this plugin.")
    }
}
