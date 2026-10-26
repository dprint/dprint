# Developing an Editor Extension

Note: dprint 0.45 introduced a `dprint lsp` command that supports formatting files over the language server protocol, which is the easiest way to add dprint to an editor that has a language client. The editor service described here is what the vscode and intellij extensions use. Unlike a language server, it's only sent a document's text when formatting it.

Editor extensions communicate with the CLI using the `dprint editor-service` subcommand. This describes schema version 6 of the editor service, which was introduced in dprint 0.65.

## dprint editor-service --info

Run this first to find out what the installed dprint supports. It doesn't need a configuration file.

```
dprint editor-service --info
```

Outputs something like:

```json
{
  "cliVersion": "0.65.0",
  "editorServiceSchemaVersions": [5, 6],
  "configSchemaUrl": "https://dprint.dev/schemas/v0.json",
  "capabilities": {}
}
```

This can be represented by the following TypeScript types:

```ts
interface EditorServiceInfo {
  cliVersion: string;
  /** The schema versions that `dprint editor-service --schema-version <version>` supports. */
  editorServiceSchemaVersions: number[];
  /** The JSON schema of the configuration file. */
  configSchemaUrl: string;
  /** What the editor service supports in addition to its schema version. There's nothing here yet. */
  capabilities: {};
}
```

Ignore properties you don't know about.

1. If the command fails or `editorServiceSchemaVersions` doesn't include the schema version the editor extension uses, output a message saying they need to update their `dprint` version.
2. If `editorServiceSchemaVersions` only has higher schema versions, output a message saying the editor extension is not compatible and they may need to update their editor extension to the latest version.

## dprint editor-service

Without `--info`, this starts a long running process that can be communicated with over stdin and stdout.

One process formats the files of a whole workspace. For each file, the editor service finds the configuration file in an ancestor directory, or uses the global configuration file when that's allowed. The configuration is read again for each request, so an edited configuration file is used for the next request without restarting the editor service.

### Executing

Run `dprint editor-service --parent-pid <provide your current process pid here> --schema-version 6`

Start one process per dprint executable (ex. when workspace folders use different versions of dprint from their `node_modules`).

Specifying `--config <path>` uses that configuration file for every file instead of finding one.

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

- Messages may have response messages, which is either way (ex. the editor service sends a show message request and the client responds to it). A response has its own new `ID`, so correlate it with the request using the message id at the start of the response body.
- Send an update settings message first.
- If an unknown message kind is received, return an error (don't ignore it).
- The JSON data of a message may have more properties in the future, so ignore properties you don't know about.

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

#### `4` - File info (Client to Service)

Gets whether a file can be formatted and the configuration file that's used for it.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface FileInfoData {
  filePath: string;
  /** The path of the notebook when the file is one of its cells, which is only formatted when the notebook would be. */
  notebookPath?: string;
  /** Allow using the global configuration file when there's no configuration file in an ancestor directory, regardless of the setting (ex. for a command the user runs). */
  allowGlobalConfig?: boolean;
}
```

Response: File info response or an Error response (ex. the configuration failed to resolve)

#### `5` - File info response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface FileInfoResponseData {
  /** The configuration file that's used for the file, which is `null` when there isn't one. */
  configFile: ConfigFileInfo | null;
  canFormat: boolean;
  /** Why the file can't be formatted. */
  reason?: NotFormattedReason;
}

interface ConfigFileInfo {
  path: string;
  /** Whether it's the global configuration file. */
  isGlobal: boolean;
}

/**
 * - `noConfigFile` - There's no configuration file in an ancestor directory and there's no global configuration file.
 * - `globalConfigNotAllowed` - There's no configuration file in an ancestor directory and the global configuration file isn't allowed to be used (ex. offer to format with it).
 * - `notMatched` - The configuration file doesn't format the file (ex. its "includes" and "excludes" don't match it).
 * - `noPlugin` - No plugin in the configuration file handles the file.
 * - `unchanged` - The file is already formatted (only for a format).
 *
 * Handle a reason you don't know about the same as `notMatched`.
 */
type NotFormattedReason = "noConfigFile" | "globalConfigNotAllowed" | "notMatched" | "noPlugin" | "unchanged";
```

Response: None

#### `6` - Format file (Client to Service)

Formats a file with the configuration file that's used for it.

Message body:

- u32 - File path byte length
- File path
- u32 - Start byte index to format
- u32 - End byte index to format
- u32 - Options byte length, which may be `0`
- JSON options:

```ts
interface FormatFileOptions {
  /** The plugin configuration to use instead of what's in the configuration file. */
  overrideConfig?: Record<string, unknown>;
  /** The path of the notebook when the file is one of its cells, which is only formatted when the notebook would be. */
  notebookPath?: string;
  /** Allow using the global configuration file when there's no configuration file in an ancestor directory, regardless of the setting (ex. for a command the user runs). */
  allowGlobalConfig?: boolean;
}
```

- u32 - File text byte length
- File text

When a file isn't formatted because there's no configuration file for it (`noConfigFile` or `globalConfigNotAllowed`), the editor service notifies the user once per session with a show message request (unless the user turned that off or dismissed it).

Response: Format file response, an Error response (ex. a formatting error or the configuration failed to resolve), or no response for a cancelled format

#### `7` - Format file response (Service to Client)

Message body:

- u32 - Message id of the request
- u32 - Response kind
  - `0` - Not formatted
    - u32 - Data byte length
    - JSON data: `{ reason: NotFormattedReason }`
  - `1` - Formatted
    - u32 - Length of formatted file text
    - Formatted file text

Response: None

#### `8` - Cancel a format (Client to Service)

Message body:

- u32 - Message id of the format to cancel

Response: Clients should not expect a message back. This message is fire and forget. Remember though, you may still receive a response from the CLI for this cancelled message. In that case, just ignore the message.

#### `9` - Configuration info (Client to Service)

Gets the plugins of the configuration file that's used for a file (ex. for the JSON schema of a configuration file).

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ConfigInfoData {
  /** A file the configuration file is used for, which may be the configuration file itself. */
  filePath: string;
  allowGlobalConfig?: boolean;
}
```

Response: Configuration info response or an Error response

#### `10` - Configuration info response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface ConfigInfoResponseData {
  /** The configuration file that's used for the file, which is `null` when there isn't one. */
  configFile: ConfigFileInfo | null;
  /** The plugins of the configuration file, which is empty when there isn't one. */
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

Response: None

#### `11` - Update settings (Client to Service)

The editor's settings and workspace folders. Send this as the first message and whenever they change.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface EditorSettings {
  /** The workspace folders open in the editor. */
  workspaceFolders?: string[];
  /** Whether to allow formatting the files that don't have a configuration file in an ancestor directory with the global configuration file. Defaults to `false`. */
  allowGlobalConfig?: boolean;
  /** Whether to notify the user when a file isn't formatted because it doesn't have a configuration file. Defaults to `true`. */
  showNoConfigNotification?: boolean;
}
```

Response: Success response or an Error response

#### `12` - Show message (Service to Client)

Shows a message to the user (ex. a notification).

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ShowMessageData {
  /** Show a kind you don't know about the same as `info`. */
  kind: "info" | "warning" | "error";
  message: string;
}
```

Response: None

#### `13` - Show message request (Service to Client)

Shows a message to the user with actions to choose from.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ShowMessageRequestData {
  kind: "info" | "warning" | "error";
  message: string;
  actions: MessageAction[];
}

interface MessageAction {
  title: string;
}
```

Response: Show message request response once the user responds, which may be a long time later. An Error response is the same as the user not choosing an action.

#### `14` - Show message request response (Client to Service)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface ShowMessageRequestResponseData {
  /** The action the user chose, which is `null` when they didn't choose one (ex. they closed the message). */
  action: MessageAction | null;
}
```

Response: None
