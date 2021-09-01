# Developing an Editor Extension (Schema Version 4)

Note: Schema version 4 was introduced in dprint 0.17

Editor extensions communicate with the CLI using the `dprint editor-info` and `dprint editor-service` subcommand.

## dprint editor-info

Called first in order to get information about the current working directory.

```
dprint editor-info
```

Outputs something like:

```
{
    "schemaVersion": 4,
    "cliVersion": "0.17.0",
    "configSchemaUrl": "https://dprint.dev/schemas/v0.json",
    "plugins":[{
        "name": "test-plugin",
        "version": "0.1.0",
        "config_key": "test",
        "fileExtensions": ["txt"],
        "fileNames": [],
        "helpUrl": "https://dprint.dev/plugins/test-plugin"
    }, {
        "name": "javascript-plugin",
        "version": "0.2.1",
        "config_key": "javascript",
        "fileExtensions": ["js"],
        "fileNames": [],
        "configSchemaUrl": "https://dprint.dev/schemas/javascript-plugin.json",
        "helpUrl": "https://dprint.dev/plugins/javascript-plugin"
    }]
}
```

1. If the `schemaVersion` number is less than the expected, output a message saying they need to update their global `dprint` version.
2. If the `schemaVersion` number is greater than the expected, output a message saying the editor extension is not compatible and they may need to update their editor extension to the latest version.

This schema can be represented by the following TypeScript types:

```ts
interface CliInfo {
  schemaVersion: number;
  cliVersion: string;
  configSchemaUrl: string;
  plugins: PluginInfo[];
}

interface PluginInfo {
  name: string;
  version: string;
  configKey: string;
  fileExtensions: string[];
  // these are exact file names the extension should format regardless of extension
  fileNames: string[];
  // will be `undefined` when the plugin does not have a schema url
  configSchemaUrl?: string;
  helpUrl: string;
}
```

## dprint editor-service

This starts a long running process that can be communicated with over stdin and stdout.

### Executing

Run `dprint editor-service --parent-pid <provide your current process pid here>`

The editor service polls for the provided process id every 30 seconds and if it doesn't exist it will exit.

### Message Kinds

After startup, send one of the following messages:

- `0` - Shutdown the process
- `1` - Check if a path can be formatted by the CLI.
- `2` - Format a file.

#### `0` - Shutting down the process

- Editor sends
  - u32 (4 bytes) - Message kind `1` indicating to shut down the process.

#### `1` - Checking a file can be formatted

- Editor sends:
  - u32 (4 bytes) - Message kind `2` indicating to check if a path can be formatted by the CLI.
  - u32 (4 bytes) - Path file size
  - X bytes - Path as string
  - <SUCCESS_BYTES>
- CLI responds:
  - u32 (4 bytes) - 0 for cannot format, or 1 for can format

#### `2` - Formatting a file

- Editor sends:
  - u32 (4 bytes) - Message kind `3` for formatting a file.
  - u32 (4 bytes) - Path file size
  - X bytes - Path as string
  - u32 (4 bytes) - File text size
  - X bytes - File text
  - <SUCCESS_BYTES>
- CLI responds:
  - u32 (4 bytes) - 0 for no change (END, no more messages), 1 for change, 2 for error
  - u32 (4 bytes) - Formatted file text or error message size
  - X bytes - Formatted file text or error message
  - <SUCCESS_BYTES>

### General

- Everything is big endian and utf-8
- Communication is always done with a buffer size of 1024. So if sending data (X bytes) above 1024 bytes then the following protocol happens:
  1. Write 1024 bytes.
  2. Wait for 4 byte ready response from CLI
  3. If there are still more than 1024 bytes to write, write 1024 bytes and go back to step 2. If not, write the remaining bytes and exit the loop.
- <SUCCESS_BYTES> - The success bytes ensures the message was received as intended. The bytes are: `255, 255, 255, 255`

If using Rust, there is a `StdIoMessenger` in dprint-core that helps with this.
