# Creating a Wasm Plugin (Schema Version 4)

Wasm plugins are the preferred way of developing plugins (as opposed to process plugins) because they are portable and run sandboxed in a Wasm runtime. They can be written in any language that supports compiling to a WebAssembly file (_.wasm_)—emscripten solutions do not work.

## Rust - Using `dprint-core`

Implementing a Wasm plugin is easier if you're using Rust as there are several helpers in `dprint-core`.

1. Use the `wasm` feature from `dprint-core` in _Cargo.toml_:

   ```toml
   dprint-core = { version = "...", features = ["wasm"] }
   serde = { version = "1.0", features = ["derive"] }
   serde_json = { version = "1.0", features = ["preserve_order"] }
   ```

2. Add the following to _Cargo.toml_:

   ```toml
   [lib]
   crate-type = ["lib", "cdylib"]
   ```

3. Create a `Configuration` struct somewhere in your project:

   ```rust
   use serde::Serialize;

   #[derive(Clone, Serialize)]
   #[serde(rename_all = "camelCase")]
   pub struct Configuration {
     // add configuration properties here...
     pub line_width: u32, // for example
   }
   ```

4. Implement `PluginHandler`:

   ```rust
   use anyhow::Result;
   use dprint_core::configuration::ConfigKeyMap;
   use dprint_core::configuration::GlobalConfiguration;
   use dprint_core::configuration::get_unknown_property_diagnostics;
   use dprint_core::configuration::get_value;
   use dprint_core::generate_plugin_code;
   use dprint_core::plugins::FileMatchingInfo;
   use dprint_core::plugins::PluginInfo;
   use dprint_core::plugins::PluginResolveConfigurationResult;
   use dprint_core::plugins::SyncPluginHandler;

   use crate::configuration::Configuration; // import the Configuration from above

   #[derive(Default)]
   pub struct MyPluginHandler;

   impl SyncPluginHandler<Configuration> for MyPluginHandler {
     fn plugin_info(&mut self) -> PluginInfo {
       PluginInfo {
         name: env!("CARGO_PKG_NAME").to_string(),
         version: env!("CARGO_PKG_VERSION").to_string(),
         config_key: "keyGoesHere".to_string(),
         help_url: "".to_string(),          // fill this in
         config_schema_url: "".to_string(), // leave this empty for now
         update_url: None,                  // leave this empty for now
       }
     }

     fn license_text(&mut self) -> String {
       "License text goes here.".to_string()
     }

     fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> PluginResolveConfigurationResult<Configuration> {
       // implement this... for example
       let mut config = config;
       let mut diagnostics = Vec::new();
       let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

       diagnostics.extend(get_unknown_property_diagnostics(config));

       PluginResolveConfigurationResult {
         config: Configuration { line_width },
         diagnostics,
         file_matching: FileMatchingInfo {
           // these can be derived from the config
           file_extensions: vec!["txt".to_string()],
           file_names: vec![],
         },
       }
     }

     fn check_config_updates(&self, message: dprint_core::plugins::CheckConfigUpdatesMessage) -> Result<Vec<dprint_core::plugins::ConfigChange>> {
       // check config updates here
     }

     fn format(
       &mut self,
       request: dprint_core::plugins::SyncFormatRequest<Configuration>,
       format_with_host: impl FnMut(dprint_core::plugins::SyncHostFormatRequest) -> dprint_core::plugins::FormatResult,
     ) -> dprint_core::plugins::FormatResult {
       // format here
     }
   }
   ```

5. Use the `generate_plugin_code` macro to generate the functions used by the plugin system to communicate with your struct:

   ```rust
   // specify the plugin struct name and then an expression to create it
   generate_plugin_code!(MyPluginHandler, MyPluginHandler::default());
   ```

6. Finally, compile with:

   ```bash
   cargo build --release --target=wasm32-unknown-unknown
   ```

### Format using other plugin

To format code using a different plugin, call the `format_with_host(file_path, file_text)` function that is exposed via the `generate_plugin_code!()` macro.

For example, this function is used by the markdown plugin to format code blocks.

## Schema Version 4 Overview

If you are not using `Rust`, then you must implement a lot of low level functionality.

## Wasm Exports

Low level communication:

- `get_shared_bytes_ptr() -> *const u8` - Called to get a pointer to the shared Wasm memory buffer.
- `clear_shared_bytes(size: u32) -> *const u8` - Called to get the plugin to clear its shared byte array and return a pointer to it.

Initialization functions:

- `dprint_plugin_version_4() -> u32` - Return `4`, but the CLI never calls this function (it only checks for it in the exports)
- `register_config(config_id: u32)` - Called when the plugin and global configuration is done transferring over. Store it somewhere.
- `release_config(config_id: u32)` - Release the config from memory.
- `get_config_diagnostics(config_id: u32) -> u32` - Called by the CLI to get the configuration diagnostics. Serialize the diagnostics as a JSON string, store it in the local bytes, and return the byte length.
- `get_resolved_config(config_id: u32) -> u32` - Called by the CLI to get the resolved configuration for display in the CLI. Serialize it as a JSON string, store it in the local bytes, and return the byte length.
- `get_license_text() -> u32` - Store the plugin's license text in the local bytes and return the byte length.
- `get_plugin_info() -> u32` - Store the plugin's JSON serialized information in the local bytes and return the byte length. The plugin info is a JSON object with the following properties:
  - `name` - String saying the plugin name.
  - `version` - Version of the plugin (ex. `"0.1.0"`)
  - `configKey` - Configuration key to use for this plugin in the dprint configuration file.
  - `fileExtensions` - An array of strings that say the file extensions this plugin supports (it should NOT have a leading period on the extension)
  - `helpUrl` - A string containing the URL to some web help.
  - `configSchemaUrl` - Return an empty string for now.

Formatting functions:

- `set_file_path()` - Called by the CLI for the plugin to take from its local byte array and store that data as the file path.
- `set_override_config()` - Possibly called by the CLI for the plugin to take from its local byte array and store that data as the format specific configuration.
- `format(config_id: u32) -> u32`
  - Return `0` when there's no change.
  - `1` when there's a change.
  - `2` when there's an error.
- `get_formatted_text() -> u32` - Plugin should put the formatted text into its local byte array and return the size of that data.
- `get_error_text() -> u32` - Plugin should put the error text into its local byte array and return the size of that data.

Optional functions:

- `check_config_updates() -> u32` - Set the shared bytes with the input. Returns the length of the output which can be read from the shared bytes.
  - Input: todo...
  - Output: todo...
- `format_range(config_id: u32, range_start: u32, range_end: u32) -> u32`
  - Response is same as `format`

### Wasm Imports

These functions are provided by the dprint CLI on the `dprint` module of the Wasm imports. They may be used for getting the CLI to format code with another plugin. The Wasm plugin must expect these otherwise the CLI will error. You don't have to implement using them though.

Communication is done by using a shared Wasm buffer. Essentially, the plugin stores its data somewhere, then writes to the shared Wasm buffer, and communicates this information to the host. The host does what the plugin tells it to do and stores its information in a local byte array.

Low level communication:

- `host_write_buffer(pointer: u32)` - Tell the host to write data to the provided Wasm memory address.

High level functions:

- `host_format(file_path_ptr: u32, file_path_len: u32, range_start: u32, range_end: u32, override_cfg_ptr: u32, override_cfg_len: u32, file_bytes_ptr: u32, file_bytes_len: u32) -> u32` - Tell the host to format using the file text in its local byte array. Provide `0` and `file_bytes_len` for no range formatting.
  - Returns `0` for no change (do nothing else, no transfer needed)
  - `1` for change (use `host_get_formatted_text()`)
  - `2` for error (use `host_get_error_text()`)
- `host_get_formatted_text() -> u32` - Tell the host to store the formatted text in its local byte array and return back the byte length of that text.
- `host_get_error_text() -> u32` - Tell the host to store the error text in its local byte array and return back the byte length of that error message.
- `host_has_cancelled() -> u32` - Check if the host has cancelled the formatting request (`1`) or not (`0`).

I recommend looking in the [`dprint-core` wasm module](https://github.com/dprint/dprint/blob/main/crates/core/src/plugins/wasm/mod.rs) for how to use these.
