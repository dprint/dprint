use serde::{Serialize, Deserialize};

/// Information about a plugin.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    /// The name of the plugin.
    pub name: String,
    /// The version of the plugin.
    pub version: String,
    /// Gets the key that can be used in the configuration JSON.
    pub config_key: String,
    /// The file extensions this plugin supports.
    pub file_extensions: Vec<String>,
    /// A url the user can go to in order to get help information about the plugin.
    pub help_url: String,
    /// Schema url for the plugin configuration.
    pub config_schema_url: String,
}

/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 1;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod macros {
    #[macro_export]
    macro_rules! generate_plugin_code {
        () => {
            // FORMATTING

            static mut FILE_PATH: Option<PathBuf> = None;
            static mut FORMATTED_TEXT: Option<String> = None;
            static mut ERROR_TEXT: Option<String> = None;

            #[no_mangle]
            pub fn set_file_path() {
                let text = take_string_from_shared_bytes();
                unsafe { FILE_PATH.replace(PathBuf::from(text)) };
            }

            #[no_mangle]
            pub fn format() -> u8 {
                ensure_initialized();
                let file_path = unsafe { FILE_PATH.take().expect("Expected the file path to be set.") };
                let file_text = take_string_from_shared_bytes();

                let formatted_text = format_text(&file_path, &file_text, &get_resolved_config_result().config);
                match formatted_text {
                    Ok(formatted_text) => {
                        if formatted_text == file_text {
                            0 // no change
                        } else {
                            unsafe { FORMATTED_TEXT.replace(formatted_text) };
                            1 // change
                        }
                    },
                    Err(err_text) => {
                        unsafe { ERROR_TEXT.replace(err_text) };
                        2 // error
                    }
                }
            }

            #[no_mangle]
            pub fn get_formatted_text() -> usize {
                let formatted_text = unsafe { FORMATTED_TEXT.take().expect("Expected to have formatted text.") };
                set_shared_bytes_str(formatted_text)
            }

            #[no_mangle]
            pub fn get_error_text() -> usize {
                let error_text = unsafe { ERROR_TEXT.take().expect("Expected to have error text.") };
                set_shared_bytes_str(error_text)
            }

            // CONFIGURATION

            static mut RESOLVE_CONFIGURATION_RESULT: Option<dprint_core::configuration::ResolveConfigurationResult<Configuration>> = None;

            #[no_mangle]
            pub fn get_plugin_info() -> usize {
                use dprint_core::plugins::PluginInfo;
                let info_json = serde_json::to_string(&PluginInfo {
                    name: String::from(env!("CARGO_PKG_NAME")),
                    version: String::from(env!("CARGO_PKG_VERSION")),
                    config_key: get_plugin_config_key(),
                    file_extensions: get_plugin_file_extensions(),
                    help_url: get_plugin_help_url(),
                    config_schema_url: get_plugin_config_schema_url(),
                }).unwrap();
                set_shared_bytes_str(info_json)
            }

            #[no_mangle]
            pub fn get_resolved_config() -> usize {
                let json = serde_json::to_string(&get_resolved_config_result().config).unwrap();
                set_shared_bytes_str(json)
            }

            #[no_mangle]
            pub fn get_config_diagnostics() -> usize {
                let json = serde_json::to_string(&get_resolved_config_result().diagnostics).unwrap();
                set_shared_bytes_str(json)
            }

            fn get_resolved_config_result<'a>() -> &'a dprint_core::configuration::ResolveConfigurationResult<Configuration> {
                unsafe {
                    ensure_initialized();
                    return RESOLVE_CONFIGURATION_RESULT.as_ref().unwrap();
                }
            }

            fn ensure_initialized() {
                unsafe {
                    if RESOLVE_CONFIGURATION_RESULT.is_none() {
                        if let Some(global_config) = GLOBAL_CONFIG.take() {
                            if let Some(plugin_config) = PLUGIN_CONFIG.take() {
                                let config_result = resolve_config(plugin_config, &global_config);
                                RESOLVE_CONFIGURATION_RESULT.replace(config_result);
                                return;
                            }
                        }

                        panic!("Plugin must have global config and plugin config set before use.");
                    }
                }
            }

            // INITIALIZATION

            static mut GLOBAL_CONFIG: Option<dprint_core::configuration::GlobalConfiguration> = None;
            static mut PLUGIN_CONFIG: Option<std::collections::HashMap<String, String>> = None;

            #[no_mangle]
            pub fn set_global_config() {
                let text = take_string_from_shared_bytes();
                let global_config: dprint_core::configuration::GlobalConfiguration = serde_json::from_str(&text).unwrap();
                unsafe {
                    GLOBAL_CONFIG.replace(global_config);
                    RESOLVE_CONFIGURATION_RESULT.take(); // clear
                }
            }

            #[no_mangle]
            pub fn set_plugin_config() {
                let text = take_string_from_shared_bytes();
                let plugin_config: std::collections::HashMap<String, String> = serde_json::from_str(&text).unwrap();
                unsafe {
                    PLUGIN_CONFIG.replace(plugin_config);
                    RESOLVE_CONFIGURATION_RESULT.take(); // clear
                }
            }

            // LOW LEVEL SENDING AND RECEIVING

            const WASM_MEMORY_BUFFER_SIZE: usize = 4 * 1024;
            static mut WASM_MEMORY_BUFFER: [u8; WASM_MEMORY_BUFFER_SIZE] = [0; WASM_MEMORY_BUFFER_SIZE];
            static mut SHARED_BYTES: Vec<u8> = Vec::new();

            #[no_mangle]
            pub fn get_plugin_schema_version() -> u32 {
                dprint_core::plugins::PLUGIN_SYSTEM_SCHEMA_VERSION // version 1
            }

            #[no_mangle]
            pub fn get_wasm_memory_buffer() -> *const u8 {
                unsafe { WASM_MEMORY_BUFFER.as_ptr() }
            }

            #[no_mangle]
            pub fn get_wasm_memory_buffer_size() -> usize {
                WASM_MEMORY_BUFFER_SIZE
            }

            #[no_mangle]
            pub fn add_to_shared_bytes_from_buffer(length: usize) {
                unsafe {
                    SHARED_BYTES.extend(&WASM_MEMORY_BUFFER[..length])
                }
            }

            #[no_mangle]
            pub fn set_buffer_with_shared_bytes(offset: usize, length: usize) {
                unsafe {
                    let bytes = &SHARED_BYTES[offset..(offset+length)];
                    &WASM_MEMORY_BUFFER[..length].copy_from_slice(bytes);
                }
            }

            #[no_mangle]
            pub fn clear_shared_bytes(capacity: usize) {
                unsafe { SHARED_BYTES = Vec::with_capacity(capacity); }
            }

            fn take_string_from_shared_bytes() -> String {
                unsafe {
                    let bytes = std::mem::replace(&mut SHARED_BYTES, Vec::with_capacity(0));
                    String::from_utf8(bytes).unwrap()
                }
            }

            fn set_shared_bytes_str(text: String) -> usize {
                let length = text.len();
                unsafe { SHARED_BYTES = text.into_bytes() }
                length
            }
        }
   }
}
