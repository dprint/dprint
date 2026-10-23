# Developing an Editor Extension (Schema Version 5)

**DO NOT USE THIS**: dprint 0.45 introduced a `dprint lsp` command that supports formatting files over the language server protocol. Please use that instead. This editor-service will remain active for the foreseeable future as it's currently used by the vscode and intellij plugins (though vscode is switching over to `dprint lsp` in the future).

Note: Schema version 5 was introduced in dprint 0.29

Editor extensions communicate with the CLI using the `dprint editor-info` and `dprint editor-service` subcommands.

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
        "configKey": "test",
        "fileExtensions": ["txt"],
        "fileNames": [],
        "helpUrl": "https://dprint.dev/plugins/test-plugin",
        "updateUrl": "https://plugins.dprint.dev/test/latest.json"
    }, {
        "name": "javascript-plugin",
        "version": "0.2.1",
        "configKey": "javascript",
        "fileExtensions": ["js"],
        "fileNames": [],
        "configSchemaUrl": "https://dprint.dev/schemas/javascript-plugin.json",
        "helpUrl": "https://dprint.dev/plugins/javascript-plugin",
        "updateUrl": "https://plugins.dprint.dev/javascript/latest.json"
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
  // will be `undefined` when the plugin does not have an update url
  updateUrl?: string;
}
```

## dprint editor-service

This starts a long running process that can be communicated with over stdin and stdout.

### Executing

Run `dprint editor-service --parent-pid <provide your current process pid here>`

The editor service checks every 5 seconds whether the provided process id still exists and if it doesn't it will exit.

### Messages

Messages are sent in the following format:

```
<ID><KIND><BODY_LENGTH>[<BODY>]<SUCCESS_BYTES>
```

- `ID` - u32 - Identifier of the message
- `KIND` - u32 - Kind of request
- `BODY_LENGTH` - u32 - Size of the body (`0` when there is no body)
- `BODY` - Depends on the kind and may be optional
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

Notes:

- Messages sent from the client to the editor service may have response messages. A response has its own new `ID`, so correlate it with the request using the message id at the start of the response body.
- If an unknown message kind is received, return an error (don't ignore it).

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

#### `3` - Active (Client to Service)

For checking if the service is healthy and can respond to messages.

Message body: None

Response: Success response

#### `4` - Can format (Client to Service)

Message body:

- u32 - File path byte length
- File path

Response: Can format response or an Error response (ex. the configuration failed to resolve)

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

Response: Format file response or an Error response (ex. a formatting error or the configuration changed while formatting)

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

#### `9` - Capabilities (Client to Service, Service to Client)

Asks the other side what it supports. Each side only sends the newer messages below to the other side when it says it supports them, so a client that never sends this message keeps working the same way. An older editor service responds with an error response because it doesn't know this message kind, which means it doesn't support anything below.

Message body:

- u32 - Capabilities byte length
- JSON capabilities of the side sending the message

Response: Capabilities response with the JSON capabilities of the side receiving the message

The capabilities are a JSON object with the following properties, which are `false` when not specified:

- `approveProcessPlugins` - Whether the "Approve process plugins" message is supported. For a client, this means it can ask the user whether to allow running process plugins.

```ts
interface EditorCapabilities {
  approveProcessPlugins?: boolean;
}
```

#### `10` - Capabilities response (Service to Client, Client to Service)

Message body:

- u32 - Message id responding to
- u32 - Capabilities byte length
- JSON capabilities

Response: None

#### `11` - Approve process plugins (Service to Client)

Process plugins aren't sandboxed, so when the user requires approving them (`"allowProcessPlugins": false` in the `permissions.json` file of the global configuration directory), the editor service doesn't set up or start the process plugins of a configuration file until the user allows them. Opening a project isn't the same as choosing to run dprint in it. This message is sent to a client that supports it to ask the user whether to allow them. It's sent at most once per session for each configuration of a configuration file. A client that doesn't support it has a warning logged to stderr instead.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ApproveProcessPluginsData {
  /** The configuration file that specifies the process plugins. */
  configFile: string;
  /** The URLs or paths of the process plugins with their checksums. */
  plugins: string[];
  /** The message to show the user. */
  message: string;
}
```

Response: Approve process plugins response once the user responds. Don't wait on the user to respond to other messages. The process plugins are used by the messages handled after the response is received.

#### `12` - Approve process plugins response (Client to Service)

Message body:

- u32 - Message id responding to
- u32 - 0 for not allowed, or 1 for allowed

Response: None
