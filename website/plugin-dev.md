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
pub struct Configuration {
    // add configuration properties here
}

pub fn resolve_config(
    config: HashMap<String, String>,
    global_config: &GlobalConfiguration,
) -> ResolveConfigurationResult<Configuration> {
    // implement...
}

fn get_plugin_config_key() -> String {
    // return the JSON object key name used in the configuration file
}

fn get_plugin_file_extensions() -> Vec<String> {
    // return the file extensions this plugin will format
}

fn get_plugin_help_url() -> String {
    // return the help url of the plugin
    // ex. https://dprint.dev/plugins/json
}

fn get_plugin_config_schema_url() -> String {
    // for now, return an empty string...
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

After implementing, compile with:

```bash
cargo build --release --target=wasm32-unknown-unknown
```

## Other Languages

If you are interested in implementing plugins in another language that supports compiling to a *.wasm* file, please [open an issue](https://github.com/dprint/dprint/issues/new?template=other.md) and I will try to help point you in the right direction.
