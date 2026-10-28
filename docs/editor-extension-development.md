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

- `ID` - u32 - Identifier of the message, which is unique among the requests a side sent that it hasn't received a response for (ex. increment it for each message)
- `KIND` - u32 - Kind of request
- `BODY_LENGTH` - u32 - Size of the body (`0` when there is no body)
- `BODY` - Depends on the kind and may be optional
- `SUCCESS_BYTES` - 4 bytes (255, 255, 255, 255)

Notes:

- Messages may have response messages, which is either way (ex. the editor service sends a show message request and the client responds to it). A response has its own new `ID`, so correlate it with the request using the message id at the start of the response body.
- A request with the id of a request that's in progress has an Error response.
- Send an initialize message first. The other messages that need it have an error response until then.
- If an unknown message kind is received, return an error (don't ignore it).
- The JSON data of a message may have more properties in the future, so ignore properties you don't know about.
- When a configuration file fails to resolve, the editor service shows the error with a show message as well as responding with an error. It's shown once until the configuration file resolves again because the editor makes requests without the user doing anything (ex. when a file is opened).
- The editor service sends what it logs as log messages, so stderr only has what isn't (ex. a crash).
- Messages are sent in the order they're made, so the log and progress messages made while handling a request are received before its response.

### Documents

Documents are identified by their uri, like in the language server protocol (ex. `file:///home/user/project/file.ts`). The plugin that formats a document is chosen by its file path, so a document that isn't a file is formatted as one:

- An untitled document (ex. `untitled:Untitled-1`) is formatted as a file in the first workspace folder, or the home directory when there isn't one.
- A notebook cell is formatted as a file in the notebook's directory when `notebookUri` is provided, which is what the jupyter plugin does when `dprint fmt` formats a notebook. It's only formatted when the notebook would be.

The file is named after the document's language (ex. `Untitled.ts` or `code_block.py`), so provide the `language` of these documents:

```ts
interface DocumentLanguage {
  /** The editor's id of the language (ex. "typescript"). */
  id: string;
  /** The file extensions the editor associates with the language with the leading period (ex. ".ts"). */
  fileExtensions?: string[];
  /** The file names the editor associates with the language (ex. "Dockerfile"). */
  fileNames?: string[];
}
```

The file name is the first of the file extensions, the file names and then the file extension dprint knows for the language id that the configuration file formats, or otherwise the first one. Requests for any other kind of uri have an Error response.

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

#### `4` - Initialize (Client to Service)

Send this as the first message.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface InitializeData {
  clientInfo: {
    /** The name of the editor (ex. "vscode"). */
    name: string;
    version?: string;
  };
  /** The settings, which are the same as the ones of an update settings message. */
  settings?: EditorSettings;
  /** What the editor supports. There's nothing here yet. */
  capabilities?: {};
}
```

Response: Success response or an Error response (ex. it was already initialized)

#### `5` - Update settings (Client to Service)

The editor's settings and workspace folders. Send this whenever they change after initializing. These replace all the settings, so send every setting each time.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface EditorSettings {
  /** The uris of the workspace folders open in the editor. */
  workspaceFolders?: string[];
  /** Whether to allow formatting the files that don't have a configuration file in an ancestor directory with the global configuration file. Defaults to `false`. */
  allowGlobalConfig?: boolean;
  /** Whether to notify the user when a file isn't formatted because it doesn't have a configuration file. Defaults to `true`. */
  showNoConfigNotification?: boolean;
  /** Whether to send debug log messages. Defaults to whether the editor service was started with `--verbose`. */
  verbose?: boolean;
}
```

Response: Success response or an Error response

#### `6` - Format file (Client to Service)

Formats a document with the configuration file that's used for it.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface FormatFileData {
  uri: string;
  /** The language of the document, which is needed for a document that isn't a file (see "Documents"). */
  language?: DocumentLanguage;
  /** The uri of the notebook when the document is one of its cells. */
  notebookUri?: string;
  /** The range of UTF-16 offsets in the text to format, which is the whole text when not provided. */
  range?: OffsetRange;
  /** The plugin configuration to use instead of what's in the configuration file. */
  overrideConfig?: Record<string, unknown>;
  /** Allow using the global configuration file when there's no configuration file in an ancestor directory, regardless of the setting (ex. for a command the user runs). */
  allowGlobalConfig?: boolean;
  /** Format the text again until the output stops changing, the same as `dprint fmt`. Defaults to `false` unless the `DPRINT_EDITOR_STABLE_FORMAT` environment variable is `1` or `true`. */
  ensureStableFormat?: boolean;
  /** Whether to notify the user when the document isn't formatted because it doesn't have a configuration file. Defaults to the `showNoConfigNotification` setting. */
  showNoConfigNotification?: boolean;
}

interface OffsetRange {
  start: number;
  end: number;
}
```

- u32 - Text byte length
- Text

The formatted text keeps the line endings of the text that was sent. For a notebook cell, the trailing whitespace of the formatted text is trimmed like the jupyter plugin does (except after a range that ends before the end of the cell).

A plugin that says it formats a range (`supportsRangeFormatting` in its plugin info) is given the range as it is and decides what to format around it (ex. the nodes it overlaps), so its edits may be outside the range. A plugin that doesn't say is given the range expanded to the whole lines it touches and only the edits within those lines are provided. A plugin that says it doesn't format a range doesn't format the document (`rangeFormattingNotSupported`).

When a document isn't formatted because there's no configuration file for it (`noConfigFile` or `globalConfigNotAllowed`), the editor service notifies the user once per session with a show message request (unless the user turned that off or dismissed it). Send `showNoConfigNotification: false` for a command the user ran that says why the document wasn't formatted itself.

When the configuration of a plugin that formats the document has problems (`configDiagnostics`), the editor service shows them to the user once until the configuration file changes because a format usually happens without the user doing anything (ex. on save).

Response: Format file response, an Error response (ex. a formatting error or the configuration failed to resolve), or no response once it's cancelled

#### `7` - Format file response (Service to Client)

Message body:

- u32 - Message id of the request
- u32 - Response kind
  - `0` - Not formatted
    - u32 - Data byte length
    - JSON data:

```ts
interface NotFormattedData {
  reason: NotFormattedReason;
  /** The message to show the user about why the document wasn't formatted (ex. for a command the user ran), which isn't provided when it's already formatted. */
  message?: string;
  /** The problems with the configuration, which are only provided for `configDiagnostics`. */
  diagnostics?: ConfigDiagnostic[];
}

/** A problem with a configuration file. */
interface ConfigDiagnostic {
  message: string;
  /** The plugin whose configuration has the problem, which isn't provided for a problem outside a plugin's configuration. */
  pluginName?: string;
  /** The property that has the problem, when there is one. */
  propertyName?: string;
  /** Where the problem is in the configuration file, which isn't provided when that's not known (ex. it's in a configuration file that it extends). */
  range?: Range;
}

/** A range in a text, which is the same as in the language server protocol. */
interface Range {
  start: Position;
  end: Position;
}

interface Position {
  /** The zero-based line. */
  line: number;
  /** The zero-based offset in UTF-16 code units from the start of the line. */
  character: number;
}
```

- `1` - Formatted
  - u32 - Data byte length
  - JSON data:

```ts
interface FormattedData {
  /** The edits of the text that was sent, which don't overlap and are in order. */
  edits: FormatEdit[];
}

/** Replaces the text between two UTF-16 offsets of the text that was sent. */
interface FormatEdit {
  start: number;
  end: number;
  newText: string;
}
```

Apply the edits only when the document is the same as the text that was sent. Applying them from the last one to the first keeps the offsets of the other ones correct.

Response: None

#### `8` - Cancel request (Client to Service)

Stops a request that's in progress (ex. a format of a document that changed since, or a request that waits on downloading plugins), which then has no response.

Message body:

- u32 - Message id of the request to cancel

Response: None. A response for the request may already be on its way, so ignore it if it arrives.

#### `9` - File info (Client to Service)

Gets whether a document can be formatted and the configuration file that's used for it.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface FileInfoData {
  uri: string;
  /** The language of the document, which is needed for a document that isn't a file (see "Documents"). */
  language?: DocumentLanguage;
  /** The uri of the notebook when the document is one of its cells. */
  notebookUri?: string;
  /** Allow using the global configuration file when there's no configuration file in an ancestor directory, regardless of the setting (ex. for a command the user runs). */
  allowGlobalConfig?: boolean;
}
```

Response: File info response or an Error response (ex. the configuration failed to resolve), or no response once it's cancelled

#### `10` - File info response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface FileInfoResponseData {
  /** The configuration file that's used for the document, which is `null` when there isn't one. */
  configFile: ConfigFileInfo | null;
  canFormat: boolean;
  /** Why the document can't be formatted. */
  reason?: NotFormattedReason;
  /** The message to show the user about why the document can't be formatted (ex. for a command the user ran). */
  message?: string;
}

interface ConfigFileInfo {
  uri: string;
  /** Whether it's the global configuration file. */
  isGlobal: boolean;
}

/**
 * - `noConfigFile` - There's no configuration file in an ancestor directory and there's no global configuration file.
 * - `globalConfigNotAllowed` - There's no configuration file in an ancestor directory and the global configuration file isn't allowed to be used (ex. offer to format with it).
 * - `notMatched` - The configuration file doesn't format the document (ex. its "includes" and "excludes" don't match it).
 * - `noPlugin` - No plugin in the configuration file handles the document.
 * - `noPlugins` - The configuration file has no plugins.
 * - `configDiagnostics` - The configuration of a plugin that formats the document has problems (only for a format).
 * - `rangeFormattingNotSupported` - A plugin that formats the document doesn't support formatting a range (only for a format).
 * - `unchanged` - The document is already formatted (only for a format).
 *
 * Handle a reason you don't know about the same as `notMatched`.
 */
type NotFormattedReason =
  | "noConfigFile"
  | "globalConfigNotAllowed"
  | "notMatched"
  | "noPlugin"
  | "noPlugins"
  | "configDiagnostics"
  | "rangeFormattingNotSupported"
  | "unchanged";
```

Response: None

#### `11` - Configuration info (Client to Service)

Gets the plugins of the configuration file that's used for a document (ex. for the JSON schema of a configuration file).

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ConfigInfoData {
  /** A document the configuration file is used for, which may be the configuration file itself. */
  uri: string;
  allowGlobalConfig?: boolean;
}
```

Response: Configuration info response or an Error response, or no response once it's cancelled

#### `12` - Configuration info response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface ConfigInfoResponseData {
  /** The configuration file that's used for the document, which is `null` when there isn't one. */
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

#### `13` - Show message (Service to Client)

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

#### `14` - Show message request (Service to Client)

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

#### `15` - Show message request response (Client to Service)

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

#### `16` - Configuration schema (Client to Service)

Gets the JSON schema to validate configuration files with, which has the configuration schemas of the plugins in them.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ConfigSchemaData {
  /** Documents that the configuration files are used for (ex. the configuration files themselves). The schema has the plugins of each one, using the first plugin with a configuration key whose schema could be downloaded. */
  uris: string[];
  allowGlobalConfig?: boolean;
}
```

Response: Configuration schema response or an Error response, or no response once it's cancelled

#### `17` - Configuration schema response (Service to Client)

Message body:

- u32 - Message id responding to
- u32 - Data byte length
- JSON data:

```ts
interface ConfigSchemaResponseData {
  /**
   * The JSON schema. The schemas of the plugins are in it instead of referenced by url because an
   * editor may only download schemas from the domains the user trusts. A plugin whose schema couldn't
   * be downloaded isn't in it, so get it again later (ex. when a configuration file changes).
   */
  schema: object;
}
```

Response: None

#### `18` - Log (Service to Client)

What the editor service logs (ex. for an output panel).

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface LogData {
  /** Treat a level you don't know about the same as `info`. Debug messages are only sent when the `verbose` setting is on. */
  level: "error" | "warn" | "info" | "debug";
  message: string;
}
```

Response: None

#### `19` - Progress (Service to Client)

The progress of something that may take a while (ex. downloading or compiling a plugin the first time it's used), which an editor can show so it doesn't look like nothing is happening.

Message body:

- u32 - Data byte length
- JSON data:

```ts
type ProgressData =
  | {
    /** Identifies the progress in the messages that follow. */
    id: number;
    kind: "begin";
    /** What's being done (ex. "Downloading https://plugins.dprint.dev/typescript-0.95.0.wasm"). */
    title: string;
    /** From 0 to 100, which isn't provided when the progress isn't known. */
    percentage?: number;
  }
  | { id: number; kind: "report"; percentage: number }
  | { id: number; kind: "end" };
```

Response: None

#### `20` - Configuration diagnostics (Service to Client)

The problems with a configuration file (ex. to show in the editor's problems panel). The editor service sends these when it resolves a configuration file and again once it checked the configuration of its plugins, which a request that uses the configuration file does.

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ConfigDiagnosticsData {
  /** The configuration file. */
  uri: string;
  /** These replace the problems sent before for the configuration file, so this is empty once they're fixed. */
  diagnostics: ConfigDiagnostic[];
}
```

Response: None

#### `21` - Configuration changed (Service to Client)

What a configuration file formats may have changed in a way the editor can't see (ex. a configuration file that it extends changed). Send the file info and configuration info messages again for the documents that use it (ex. to update which documents the editor formats with dprint).

Message body:

- u32 - Data byte length
- JSON data:

```ts
interface ConfigChangedData {
  /** The configuration file. */
  uri: string;
}
```

Response: None
