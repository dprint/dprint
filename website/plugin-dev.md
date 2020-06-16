---
title: Creating a Plugin
description: Documentation on creating your own dprint formatting plugin.
---

# Creating a Plugin

Plugins can be written in any language that supports compiling to a WebAssembly file (*.wasm*).

## Rust

Here's an example Rust plugin created with the `generate_plugin_code` macro from [`dprint-core`](https://crates.io/crates/dprint-core).

```rust
use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::configuration::{
    GlobalConfiguration,
    ResolveConfigurationResult,
    get_unknown_property_diagnostics,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    // add configuration properties here
}

fn resolve_config(
    config: HashMap<String, String>,
    global_config: &GlobalConfiguration,
) -> ResolveConfigurationResult<Configuration> {
    // implement...
}

fn get_plugin_config_key() -> String {
    // return the JSON object key name used in the configuration file
    // ex. String::from("json")
}

fn get_plugin_file_extensions() -> Vec<String> {
    // return the file extensions this plugin will format
    // ex. vec![String::from("json")]
}

fn get_plugin_help_url() -> String {
    // return the help url of the plugin
    // ex. String::from("https://dprint.dev/plugins/json")
}

fn get_plugin_config_schema_url() -> String {
    // for now, return an empty string. Return a schema url once VSCode
    // supports $schema properties in descendant objects:
    // https://github.com/microsoft/vscode/issues/98443
    String::new()
}

fn get_plugin_license_text() -> String {
    std::str::from_utf8(include_bytes!("../LICENSE")).unwrap().into()
}

fn format_text(
    file_path: &PathBuf,
    file_text: &str,
    config: &Configuration,
) -> Result<String, String> {
    // implement...
}

generate_plugin_code!();
```

Add the following to *Cargo.toml*:

```toml
[lib]
crate-type = ["lib", "cdylib"]
```

Then finally, compile with:

```bash
cargo build --release --target=wasm32-unknown-unknown
```

## Other Languages

If you are interested in implementing plugins in another language that supports compiling to a *.wasm* file, please [open an issue](https://github.com/dprint/dprint/issues/new?template=other.md) and I will try to help point you in the right direction.
