# Developing an Editor Extension (Schema Version 5)

Note: Schema version 5 was introduced in dprint 0.25

Editor extensions communicate with the CLI using the `dprint editor-info` and `dprint editor-service` subcommand.

## dprint editor-info

Called first in order to get information about the current working directory.

```
dprint editor-info
```

Outputs something like:

```
{
    "schemaVersion": 5,
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

The editor service polls for the provided process id every 10 seconds and if it doesn't exist it will exit.

### Messages

Messages are sent in the following format:

```
<ID><KIND>[<BODY>]<SUCCESS_BYTES>
```

- `ID` - u32 - Identifier of the message.
- `KIND` - u32 - Kind of request
- `BODY` - Depends on the kind and may be optional
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

Messages sent from the client to the editor service may have response messages and responses need to be correlated with the ID of the message that was sent.

### Message Kinds

#### `0` - Success Response (Service to Client, Client to Service)

Message body:

- u32 - Message id that succeeded.

Response: No response

#### `1` - Error Response (Service to Client, Client to Service)

Message body:

- u32 - Message id that failed.
- u32 - Error message byte length
- X bytes - Error message

Response: No response

#### `2` - Shut down the process (Client to Service)

Causes the service to shut down itself and all the process plugins gracefully.

Message body: None

Response: Success response and then CLI will exit process. The CLI will handle the client not accepting this response though.

#### `3` - Active (Client to Service, Service to Client)

For checking if the service is healthy and can respond to messages.

Message body: None

Response: Success response

#### `4` - Can format (Client to Service)

Message body:

- u32 - File path byte length
- File path

Response: Can format response

#### `5` - Can format response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - 0 for cannot format, or 1 for can format

Response: None

#### `6` - Format file (Client to Service)

Message body:

- u32 - File path content byte length
- File path
- u32 - Start byte index to format
- u32 - End byte index to format
- u32 - Override configuration byte length
- JSON override configuration
- u32 - File text content byte length
- File text

Response: Format file response

#### `7` - Format file response (Service to Client)

Message body:

- u32 - Message id of the request
- u32 - Response Kind
  - `0` - No Change
  - `1` - Change
    - u32 - Length of formatted file text
    - Formatted file text

Response: None

#### `8` - Cancel a format (Client to Service)

Message body:

- u32 - Message id of the format to cancel

Response: Clients should not expect a message back. This message is fire and forget. Remember though, you may still receive a response from the CLI for this cancelled message. In that case, just ignore the message.
