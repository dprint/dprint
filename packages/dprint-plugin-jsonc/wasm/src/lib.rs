#![allow(non_snake_case)] // allow for js property names

extern crate console_error_panic_hook;
extern crate dprint_plugin_jsonc;

use dprint_core::configuration::*;
use dprint_plugin_jsonc::configuration::*;
use dprint_plugin_jsonc::format_text;
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
pub struct FormatContext {
    configuration: Configuration,
    diagnostics: Vec<ConfigurationDiagnostic>,
}

#[wasm_bindgen]
impl FormatContext {
    pub fn new(js_config: &js_sys::Map, js_global_config: &js_sys::Map) -> FormatContext {
        console_error_panic_hook::set_once();

        let global_config = resolve_global_config(js_map_to_hash_map(&js_global_config)).config;
        let config_result = resolve_config(js_map_to_hash_map(&js_config), &global_config);
        FormatContext {
            configuration: config_result.config,
            diagnostics: config_result.diagnostics,
        }
    }

    /// Gets the JSON serialized configuration.
    pub fn get_configuration(&self) -> String {
        serde_json::to_string(&self.configuration).unwrap()
    }

    /// Gets the JSON serialized configuration diagnostics.
    pub fn get_configuration_diagnostics(&self) -> String {
        serde_json::to_string(&self.diagnostics).unwrap()
    }

    pub fn format(&self, file_text: &str) -> Result<Option<String>, JsValue> {
        match format_text(file_text, &self.configuration) {
            Ok(result) => {
                Ok(if result == file_text {
                    None
                } else {
                    Some(result)
                })
            },
            Err(result) => Err(JsValue::from(result))
        }
    }
}

fn js_map_to_hash_map(map: &js_sys::Map) -> HashMap<String, String> {
    let mut hash_map = HashMap::new();
    for key in map.keys() {
        let key = key.unwrap();
        let value = map.get(&key);
        let key = key.as_string().unwrap();
        if let Some(value) = value.as_string() {
            hash_map.insert(key, value);
        }
    }
    hash_map
}
