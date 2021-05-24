use jsonc_parser::{parse_to_value, JsonValue, JsonObject, JsonArray};
use dprint_core::types::ErrBox;

use crate::environment::Environment;

#[derive(PartialEq, Debug)]
pub struct InfoFile {
    pub plugin_system_schema_version: u32,
    pub latest_plugins: Vec<InfoFilePluginInfo>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct InfoFilePluginInfo {
    pub name: String,
    pub version: String,
    pub url: String,
    pub config_schema_url: String,
    pub config_key: Option<String>,
    pub file_extensions: Vec<String>,
    pub config_excludes: Vec<String>,
    pub is_process_plugin: bool,
    pub checksum: Option<String>,
}

const SCHEMA_VERSION: u8 = 2;
pub const REMOTE_INFO_URL: &'static str = "https://plugins.dprint.dev/info.json";

pub fn read_info_file(environment: &impl Environment) -> Result<InfoFile, ErrBox> {
    let info_bytes = environment.download_file(REMOTE_INFO_URL)?;
    let info_text = String::from_utf8(info_bytes.to_vec())?;
    let json_value = parse_to_value(&info_text)?;
    let mut obj = match json_value {
        Some(JsonValue::Object(obj)) => obj,
        _ => return err!("Expected object in root element."),
    };

    // check schema version
    let schema_version = match obj.take_number("schemaVersion") {
        Some(value) => value.parse::<u32>()?,
        _ => return err!("Could not find schema version."),
    };
    if schema_version != SCHEMA_VERSION as u32 {
        return err!("Cannot handle schema version {}. Expected {}. This might mean your dprint CLI version is old and isn't able to get the latest information.", schema_version, SCHEMA_VERSION);
    }

    // get plugin system version
    let plugin_system_schema_version = match obj.take_number("pluginSystemSchemaVersion") {
        Some(value) => value.parse::<u32>()?,
        _ => return err!("Could not find plugin system schema version."),
    };

    let latest_plugins = match obj.take_array("latest") {
        Some(arr) => {
            let mut plugins = Vec::new();
            for value in arr.into_iter() {
                plugins.push(get_latest_plugin(value)?);
            }
            plugins
        },
        _ => return err!("Could not find latest plugins array."),
    };

    Ok(InfoFile {
        plugin_system_schema_version,
        latest_plugins,
    })
}

fn get_latest_plugin(value: JsonValue) -> Result<InfoFilePluginInfo, ErrBox> {
    let mut obj = match value {
        JsonValue::Object(obj) => obj,
        _ => return err!("Expected an object in the latest array."),
    };
    let name = get_string(&mut obj, "name")?;
    let version = get_string(&mut obj, "version")?;
    let url = get_string(&mut obj, "url")?;
    let config_key = obj.take_string("configKey").map(|k| k.into_owned());
    let config_schema_url = obj.take_string("configSchemaUrl").map(|s| s.into_owned()).unwrap_or(String::new());
    let file_extensions = get_string_array(&mut obj, "fileExtensions")?;
    let config_excludes = get_string_array(&mut obj, "configExcludes")?;
    let is_process_plugin = obj.take_boolean("isProcessPlugin").unwrap_or(false);
    let checksum = obj.take_string("checksum").map(|s| s.into_owned());

    Ok(InfoFilePluginInfo {
        name,
        version,
        url,
        config_key,
        file_extensions,
        config_schema_url,
        config_excludes,
        is_process_plugin,
        checksum,
    })
}

fn get_string_array(value: &mut JsonObject, key: &str) -> Result<Vec<String>, ErrBox> {
    let mut result = Vec::new();
    for item in get_array(value, key)? {
        match item {
            JsonValue::String(item) => result.push(item.into_owned()),
            _ => return err!("Unexpected non-string in {} array.", key),
        }
    }
    Ok(result)
}

fn get_string(value: &mut JsonObject, name: &str) -> Result<String, ErrBox> {
    match value.take_string(name) {
        Some(text) => Ok(text.into_owned()),
        _ => return err!("Could not find string: {}", name),
    }
}

fn get_array<'a>(value: &mut JsonObject<'a>, name: &str) -> Result<JsonArray<'a>, ErrBox> {
    match value.take_array(name) {
        Some(arr) => Ok(arr),
        _ => return err!("Could not find array: {}", name),
    }
}

#[cfg(test)]
mod test {
    use crate::environment::TestEnvironment;
    use super::*;

    #[test]
    fn should_get_info() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 2,
    "pluginSystemSchemaVersion": 3,
    "latest": [{
        "name": "dprint-plugin-typescript",
        "version": "0.17.2",
        "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
        "configKey": "typescript",
        "fileExtensions": ["ts", "tsx"],
        "configExcludes": ["**/node_modules"]
    }, {
        "name": "dprint-plugin-jsonc",
        "version": "0.2.3",
        "url": "https://plugins.dprint.dev/json-0.2.3.wasm",
        "fileExtensions": ["json"],
        "configSchemaUrl": "https://plugins.dprint.dev/schemas/json-v1.json",
        "configExcludes": ["**/*-lock.json"],
        "isProcessPlugin": true,
        "checksum": "test-checksum"
    }]
}"#.as_bytes());
        let info_file = read_info_file(&environment).unwrap();
        assert_eq!(info_file, InfoFile {
            plugin_system_schema_version: 3,
            latest_plugins: vec![InfoFilePluginInfo {
                name: String::from("dprint-plugin-typescript"),
                version: String::from("0.17.2"),
                url: String::from("https://plugins.dprint.dev/typescript-0.17.2.wasm"),
                config_key: Some(String::from("typescript")),
                file_extensions: vec![String::from("ts"), String::from("tsx")],
                config_schema_url: String::new(),
                config_excludes: vec![String::from("**/node_modules")],
                is_process_plugin: false,
                checksum: None,
            }, InfoFilePluginInfo {
                name: String::from("dprint-plugin-jsonc"),
                version: String::from("0.2.3"),
                url: String::from("https://plugins.dprint.dev/json-0.2.3.wasm"),
                config_key: None,
                file_extensions: vec![String::from("json")],
                config_schema_url: String::from("https://plugins.dprint.dev/schemas/json-v1.json"),
                config_excludes: vec![String::from("**/*-lock.json")],
                is_process_plugin: true, // lies for testing purposes
                checksum: Some("test-checksum".to_string()),
            }],
        })
    }

    #[test]
    fn should_error_if_schema_version_is_different() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 6,
}"#.as_bytes());
        let message = read_info_file(&environment).err().unwrap();
        assert_eq!(
            message.to_string(),
            "Cannot handle schema version 6. Expected 2. This might mean your dprint CLI version is old and isn't able to get the latest information."
        );
    }

    #[test]
    fn should_error_if_no_plugin_system_set() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 2,
}"#.as_bytes());
        let message = read_info_file(&environment).err().unwrap();
        assert_eq!(
            message.to_string(),
            "Could not find plugin system schema version."
        );
    }

    #[test]
    fn should_error_when_no_internet() {
        let environment = TestEnvironment::new();
        let message = read_info_file(&environment).err().unwrap();
        assert_eq!(
            message.to_string(),
            "Could not find file at url https://plugins.dprint.dev/info.json"
        );
    }
}