# Creating a Process Plugin (Schema Version 4)

Process plugins are created (as opposed to the recommended Wasm plugins), when the language does not have good support for compiling to a single _.wasm_ file.

## Rust - Using `dprint-core`

Implementing a Process plugin is easy if you're using Rust as there are several helpers in `dprint-core`.

1. Use the `process` feature from `dprint-core` in _Cargo.toml_:

   ```toml
   dprint-core = { version = "...", features = ["process"] }
   tokio = { version = "1", features = ["full"] } # todo: reduce features
   tokio-util = { version = "0.7.0" }
   serde = { version = "1.0.117", features = ["derive"] }
   serde_json = { version = "1.0", features = ["preserve_order"] }
   ```

2. Create a `Configuration` struct somewhere in your project:

   ```rust
   use serde::Deserialize;
   use serde::Serialize;

   #[derive(Clone, Serialize, Deserialize)]
   #[serde(rename_all = "camelCase")]
   pub struct Configuration {
     // add configuration properties here...
     line_width: u32, // for example
   }
   ```

3. Implement `AsyncPluginHandler`

   ```rust
   use std::collections::HashMap;
   use std::path::PathBuf;

   use anyhow::Result;
   use dprint_core::configuration::get_unknown_property_diagnostics;
   use dprint_core::configuration::get_value;
   use dprint_core::configuration::ConfigKeyMap;
   use dprint_core::configuration::GlobalConfiguration;
   use dprint_core::configuration::ResolveConfigurationResult;
   use dprint_core::plugins::AsyncPluginHandler;
   use dprint_core::plugins::BoxFuture;
   use dprint_core::plugins::FormatRequest;
   use dprint_core::plugins::FormatResult;
   use dprint_core::plugins::Host;
   use dprint_core::plugins::PluginInfo;

   use super::configuration::Configuration; // import the Configuration from above somehow

   pub struct MyPluginHandler;

   impl AsyncPluginHandler for MyPluginHandler {
     type Configuration = Configuration;

     fn plugin_info(&self) -> PluginInfo {
       PluginInfo {
         name: env!("CARGO_PKG_NAME").to_string(),
         version: env!("CARGO_PKG_VERSION").to_string(),
         config_key: "keyGoesHere".to_string(),
         file_extensions: vec!["txt_ps".to_string()],
         file_names: vec![],
         help_url: "".to_string(),          // ex. https://dprint.dev/plugins/prettier
         config_schema_url: "".to_string(), // the schema url for your config file
         update_url: Some(None),            // ex. https://plugins.dprint.dev/dprint/dprint-plugin-prettier/latest.json
       }
     }

     fn license_text(&self) -> String {
       // include your license file somehow
       include_str!("../LICENSE").to_string()
     }

     fn resolve_config(&self, config: ConfigKeyMap, global_config: GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
       // implement this... for example
       let mut config = config;
       let mut diagnostics = Vec::new();
       let line_width = get_value(&mut config, "line_width", global_config.line_width.unwrap_or(120), &mut diagnostics);

       diagnostics.extend(get_unknown_property_diagnostics(config));

       ResolveConfigurationResult {
         config: Configuration { ending, line_width },
         diagnostics,
       }
     }

     fn format(&self, request: FormatRequest<Self::Configuration>, host: Arc<dyn Host>) -> BoxFuture<FormatResult> {
       Box::pin(async move {
         // format here
         //
         // - make sure to check the range of the request!! If you can't handle
         //   range formatting, then return `Ok(None)` to signify no change.
         // - use `host.format` to format with another plugin
         // - if you are doing a lot of synchronous work, it's probably better
         //   to format with a blocking task like so (especially if you're using
         //   a tokio single threaded runtime):
         //
         //   tokio:task::spawn_blocking(move || {
         //     // format in here
         //   }).await.unwrap()
       })
     }
   }
   ```

4. In your plugin's `main` function, parse out the `--parent-pid` argument and using that argument, start a thread that periodically checks for the existence of that process. When the process no longer exists, then it should exit the current process. This helps prevent a process from running without ever closing. Implementing this is easy with `dprint-core` as you just need to run the `start_parent_process_checker_task` function:

   <!-- dprint-ignore -->
   ```rust
   use dprint_core::plugins::process::get_parent_process_id_from_cli_args;
   use dprint_core::plugins::process::handle_process_stdio_messages;
   use dprint_core::plugins::process::start_parent_process_checker_task;

   #[tokio::main]
   async fn main() -> Result<()> {
     if let Some(parent_process_id) = get_parent_process_id_from_cli_args() {
       start_parent_process_checker_task(parent_process_id);
     }

     handle_process_stdio_messages(MyPluginHandler).await
   }
   ```

5. Finally, use your created plugin handler to start reading and writing to stdin and stdout:

   <!-- dprint-ignore -->
   ```rust
   handle_process_stdio_messages(MyPluginHandler).await
   ```

## Schema Version 4 Overview (Not Yet Released)

Process plugins are expected to read and respond to messages on a single thread, then spawn formatting threads/tasks for doing concurrent formatting.

### Schema Version Establishment

To maintain compatibility with past dprint clients, an initial schema version establishment phase occurs that is the same as past schema versions.

1. An initial `0` (4 bytes) is sent asking for the schema version.
2. At this point, the client responds with `0` (4 bytes) for success, then `4` (4 bytes) for the schema version.

### Messages

Messages are sent from the client to the plugin in the following format:

```
<ID><KIND>[<BODY>]<SUCCESS_BYTES>
```

- `ID` - u32 - Identifier of the message. This should be an independently incrementing value on both the CLI and in the plugin.
- `KIND` - u32 - Kind of request
- `BODY` - Depends on the kind and may be optional
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

### Message Kinds

If a plugin encounters an unknown message kind, it should send an error message for the received message and exit the process.

#### `0` - Success Response (Plugin to CLI, CLI to Plugin)

Message body:

- u32 - Message id that succeeded.

Response: No response

#### `1` - Data Response (Plugin to CLI)

Message body:

- u32 - Message id that succeeded.
- u32 - Content length
- Content bytes

Response: No response

#### `2` - Error Response (Plugin to CLI, CLI to Plugin)

Message body:

- u32 - Message id of the message that failed.
- u32 - Error message byte length
- X bytes - Error message

Response: No response

#### `3` - Shut down the process (CLI to Plugin)

Causes the process to shut down gracefully.

Message body: None

Response: No response

#### `4` - Active (CLI to Plugin, Plugin to CLI)

Used to tell if the other is healthy and can respond to messages.

Response: Success message

#### `5` - Get Plugin Info (CLI to Plugin)

Message body: None

Response: Data message - JSON serialized plugin info

#### `6` - Get License Text (CLI to Plugin)

Message body: None

Response: Data message

#### `7` - Register Configuration (CLI to Plugin)

Stores configuration in memory in the process plugin. The identifier of the configuration is the request identifier.

Message body:

- u32 - Config id
- u32 - Content length
- JSON serialized global configuration
- u32 - Content length
- JSON serialized plugin configuration

Response: Success message

#### `8` - Release Configuration (CLI to Plugin)

Releases configuration from memory in the process plugin.

Message body:

- u32 - Config id

Response: Success message

#### `9` - Get Configuration Diagnostics (CLI to Plugin)

Message body:

- u32 - Config id

Response: Data message - JSON serialized diagnostics

#### `10` - Get Resolved Configuration (CLI to Plugin)

Message body:

- u32 - Config id

Response: Data message - JSON serialized resolved config

#### `11` - Format Text (CLI to Plugin)

Message body:

- u32 - File path content length
- File path
- u32 - Start byte index to format
- u32 - End byte index to format
- u32 - Configuration identifier
- u32 - Override configuration length -- TODO: Is this necessary anymore?
- JSON override configuration
- u32 - File text content length
- File text

Response: Format text response

#### `12` - Format Text Response (Plugin to CLI, CLI to Plugin)

Message body:

- u32 - Message id being responded to.
- u32 - Response Kind
  - `0` - No Change
  - `1` - Change
    - u32 - Content length of the changed text
    - Formatted file text

Response: None

#### `13` - Cancel Format (CLI to Plugin or Plugin to CLI)

Message body:

- u32 - Message id of the format to cancel

Response: No response should be given. Cancellation is not guaranteed to happen and
the CLI or plugin may still respond with a given request.

#### `14` - Host Format (Plugin to CLI)

Message body:

- u32 - Size of the file path
- File path
- u32 - Start byte index to format
- u32 - End byte index to format
- u32 - Size of the override configuration
- JSON serialized override configuration
- u32 - Size of the file text
- File text.

Response: Format Text Response

### Creating a `.exe-plugin` file

TODO...
