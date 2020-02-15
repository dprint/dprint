#![allow(non_snake_case)] // allow for js property names

extern crate console_error_panic_hook;
extern crate dprint_plugin_typescript;
extern crate dprint_core;

use dprint_core::configuration::*;
use dprint_plugin_typescript::configuration::*;
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
pub fn resolve_config(configuration: &js_sys::Map) -> String {
    serde_json::to_string(&resolve_to_typescript_config(configuration)).unwrap()
}

#[wasm_bindgen]
pub fn format_text(file_text: &str, configuration: &js_sys::Map) -> String {
    let configuration = resolve_to_typescript_config(&configuration);
    match dprint_plugin_typescript::format_text("./file.tsx", file_text, &configuration) {
        Ok(result) => match result {
            Some(result) => result,
            None => String::from(file_text),
        },
        Err(error) => String::from(error),
    }
}

fn resolve_to_typescript_config(configuration: &js_sys::Map) -> Configuration {
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
    return dprint_plugin_typescript::configuration::resolve_config(&hash_map, &global_config).config;
}