# Creating a Process Plugin (Schema Version 3)

Process plugins are created (as opposed to the recommended Wasm plugins), when the language does not have good support for compiling to a single _.wasm_ file.

## Rust - Using `dprint-core`

Implementing a Process plugin is easy if you're using Rust as there are several helpers in `dprint-core`.

1. Use the `process` feature from `dprint-core` in _Cargo.toml_:

   ```toml
   dprint-core = { version = "...", features = ["process"] }
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

3. Implement `PluginHandler`

   ```rust
   use std::collections::HashMap;
   use std::path::PathBuf;

   use anyhow::Result;
   use dprint_core::configuration::get_unknown_property_diagnostics;
   use dprint_core::configuration::get_value;
   use dprint_core::configuration::ConfigKeyMap;
   use dprint_core::configuration::GlobalConfiguration;
   use dprint_core::configuration::ResolveConfigurationResult;
   use dprint_core::plugins::PluginHandler;
   use dprint_core::plugins::PluginInfo;

   use super::configuration::Configuration; // import the Configuration from above somehow

   pub struct MyPluginHandler {}

   impl MyPluginHandler {
     fn new() -> Self {
       MyPluginHandler {}
     }
   }

   impl PluginHandler<Configuration> for MyPluginHandler {
     fn get_plugin_info(&mut self) -> PluginInfo {
       PluginInfo {
         name: env!("CARGO_PKG_NAME").to_string(),
         version: env!("CARGO_PKG_VERSION").to_string(),
         config_key: "keyGoesHere".to_string(),
         file_extensions: vec!["txt_ps".to_string()],
         file_names: vec![],
         help_url: "".to_string(),          // fill this in
         config_schema_url: "".to_string(), // leave this empty for now
       }
     }

     fn get_license_text(&mut self) -> String {
       "License text goes here.".to_string()
     }

     fn resolve_config(&mut self, config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
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

     fn format_text(
       &mut self,
       file_path: &Path,
       file_text: &str,
       config: &Configuration,
       mut format_with_host: impl FnMut(&Path, String, &ConfigKeyMap) -> Result<String>,
     ) -> Result<String> {
       // format here
     }
   }
   ```

4. In your plugin's `main` function, parse out the `--parent-pid` argument and using that argument, start a thread that periodically checks for the existence of that process. When the process no longer exists, then it should exit the current process. This helps prevent a process from running without ever closing. Implementing this is easy with `dprint-core` as you just need to run the `start_parent_process_checker_thread` function:

   <!-- dprint-ignore -->
   ```rust
   use dprint_core::plugins::process::start_parent_process_checker_thread;

   let parent_process_id = ...; // parse this from the `--parent-pid` command line argument
   start_parent_process_checker_thread(String::from(env!("CARGO_PKG_NAME")), parent_process_id);
   ```

5. Finally, use your created plugin handler to start reading and writing to stdin and stdout:

   <!-- dprint-ignore -->
   ```rust
   handle_process_stdio_messages(MyPluginHandler::new())
   ```

## Schema Version 4 Overview (Not Yet Released)

Process plugins are expected to read and respond to messages on a single thread, then spawn formatting threads/tasks for doing concurrent formatting.

### Schema Version Establishment

To maintain compatibility with past dprint clients, an initial schema version establishment phase occurs that is the same as past schema versions.

1. An initial `0` (4 bytes) is sent asking for the schema version.
2. At this point, the client responds with `0` (4 bytes) for success, then `4` (4 bytes) for the schema version.

After this point, the schema version can no longer be asked for.

### Requests

Requests are sent from the client to the plugin in the following format:

```
<ID><KIND>[<BODY>]<SUCCESS_BYTES>
```

- `ID` - u32 (4 bytes) - Number for the request or the ID of the host format request.
- `KIND` - u32 (4 bytes) - Kind of request.
- `BODY` - Depends on the kind and may be optional.
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

### Responses

Responses are sent from the plugin to the client and could include format requests.

```
<ID><KIND><BODY><SUCCESS_BYTES>
```

- `ID` - Which request this response is for or a new ID for a host format request.
- `KIND` - u32 (4 bytes) - `0` for success, `1` for failure, `2` for host format request.
- `BODY`
  - When `KIND` is `0`:
    - Depends on the request kind.
  - When `KIND` is `1`:
    - u32 (4 bytes) - Error message size
    - X bytes - Error message
  - When `KIND` is `2`:
    - u32 (4 bytes) - Size of the file path.
    - File path.
    - u32 (4 bytes) - Size of the file text.
    - File text.
    - u32 (4 bytes) - Size of the override configuration.
    - JSON serialized override configuration.
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

### Request Kinds

#### `1` - Close

Causes the process to shut down gracefully.

#### `2` - Get Plugin Info

Response body:

- u32 (4 bytes) - Content length
- JSON serialized plugin information

#### `3` - Get License Text

Response body:

- u32 (4 bytes) - Content length
- License text

#### `4` - Register Configuration

Stores configuration in memory in the process plugin.

Request body:

- u32 (4 bytes) - Content length
- JSON serialized global configuration
- u32 (4 bytes) - Content length
- JSON serialized plugin configuration

Response body:

- u32 (4 bytes) - Identifier for this configuration.

#### `5` - Release Configuration

Releases configuration from memory in the process plugin.

Request body:

- u32 (4 bytes) - Identifier for the configuration.

Response body: None

#### `6` - Get Configuration Diagnostics

Request body:

- u32 (4 bytes) - Identifier for the configuration to get diagnostics for.

Response body:

- u32 (4 bytes) - Content length
- JSON serialized array of diagnostics

#### `7` - Get Resolved Configuration

Request body:

- u32 (4 bytes) - Identifier for the configuration to get diagnostics for.

Response body:

- u32 (4 bytes) - Content length
- JSON serialized resolved configuration

#### `8` - Format Text

Request body:

- u32 (4 bytes) - File path content length
- File path
- u32 (4 bytes) - File text content length
- File text
- u32 (4 bytes) - Configuration identifier
- u32 (4 bytes) - Override configuration -- TODO: Is this necessary anymore?
- JSON override configuration

Response body:

- u32 (4 bytes) - Response Kind
  - `0` - No Change
  - `1` - Change
    - u32 (4 bytes) - Content length.
    - Formatted file text

#### `9` - Cancel Format

The request should use the same identifier as the format request.

There is no response sent for this though the plugin may respond with a

#### `10` - Host Format Request Response

The response should use the same identifier as the host formatting request.

Request body:

- u32 (4 bytes) - Response Kind
  - `0` - No Change
  - `1` - Change
    - u32 (4 bytes) - Content length.
    - Formatted file text

### Creating a `.exe-plugin` file

TODO...
