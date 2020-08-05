---
title: Creating a Plugin
description: Documentation on creating your own dprint formatting plugin.
---

# Creating a Plugin

As outlined in [plugins](/plugins), there are WASM plugins and process plugins.

* WASM plugins can be written in any language that supports compiling to a WebAssembly file (_.wasm_) (highly recommended)
* Process plugins can be written in any language that supports compiling to an executable.

## Rust (WASM Plugin)

Here's an example Rust WASM plugin created with the `generate_plugin_code` macro from [`dprint-core`](https://crates.io/crates/dprint-core).

```rust
use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::generate_plugin_code;
use dprint_core::configuration::{
    GlobalConfiguration,
    ResolveConfigurationResult,
    get_unknown_property_diagnostics,
    ConfigurationDiagnostic,
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

Add the following to _Cargo.toml_:

```toml
[lib]
crate-type = ["lib", "cdylib"]
```

Then finally, compile with:

```bash
cargo build --release --target=wasm32-unknown-unknown
```

### Format using other plugin

To format code using a different plugin, call the `format_with_host(file_path, file_text)` method that is exposed via the `generate_plugin_code!()` macro.

For example, this function is used by the markdown plugin to format code blocks.

## Process Plugins

Please [open an issue](https://github.com/dprint/dprint/issues/new?template=other.md) asking me to outline some details and I will when I can.

## Other Languages

If you are interested in implementing plugins in another language, please [open an issue](https://github.com/dprint/dprint/issues/new?template=other.md) and I will try to help point you in the right direction.
