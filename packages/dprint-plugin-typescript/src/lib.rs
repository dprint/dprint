#![allow(non_snake_case)] // allow for js property names

extern crate console_error_panic_hook;
extern crate dprint_plugin_typescript;

use dprint_plugin_typescript::*;
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
pub struct FormatContext {
    configuration: TypeScriptConfiguration,
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

        // todo: store configuration diagnostics
        let configuration = resolve_config(&hash_map);
        FormatContext {
            configuration,
        }
    }

    pub fn format(&self, file_path: &str, file_text: &str) -> Result<Option<String>, JsValue> {
        match dprint_plugin_typescript::format_text(file_path, file_text, &self.configuration) {
            Ok(result) => Ok(result),
            Err(result) => Err(JsValue::from(result))
        }
    }
}
