#![allow(non_snake_case)] // allow for js property names

extern crate console_error_panic_hook;
extern crate dprint_plugin_typescript;

use dprint_core::configuration::*;
use dprint_plugin_typescript::configuration::*;
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
pub struct FormatContext {
    configuration: Configuration,
    diagnostics: Vec<ConfigurationDiagnostic>,
}

#[wasm_bindgen]
impl FormatContext {
    pub fn new(configuration: &js_sys::Map) -> FormatContext {
        console_error_panic_hook::set_once();

        let mut hash_map = HashMap::new();
        for key in configuration.keys() {
            let key = key.unwrap();
            let value = configuration.get(&key);
            let key = key.as_string().unwrap();
            if let Some(value) = value.as_string() {
                hash_map.insert(key, value);
            }
        }

        let global_config = resolve_global_config(&HashMap::new()).config;
        let config_result = resolve_config(&hash_map, &global_config);
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

    pub fn format(&self, file_path: &str, file_text: &str) -> Result<Option<String>, JsValue> {
        match dprint_plugin_typescript::format_text(file_path, file_text, &self.configuration) {
            Ok(result) => Ok(result),
            Err(result) => Err(JsValue::from(result))
        }
    }
}
