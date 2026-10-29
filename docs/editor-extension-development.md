# Developing an Editor Extension

An editor extension formats documents with the `dprint lsp` command, which is a language server. A language client formats documents with the text document methods of the language server protocol (see [the language server docs](../website/src/lsp.md)). An editor extension uses the `dprint/*` methods described here instead, which are only sent a document's text when formatting it, so the server doesn't keep the documents. They also say which documents dprint formats and let the editor extension ask the user what needs to be approved.

The `dprint/*` methods were introduced in dprint 0.65. Editor extensions for older versions used `dprint editor-service` (see [schema version 5](./editor-extension-development-schema-5.md)), which formats the files of a configuration file that the editor found.

## Starting the server

Run `dprint lsp` in the workspace folder and send the `initialize` request with the `dprint` experimental capability:

```json
{
  "processId": 1234,
  "capabilities": {
    "experimental": { "dprint": {} },
    "window": { "workDoneProgress": true }
  },
  "workspaceFolders": [{ "uri": "file:///home/user/project", "name": "project" }],
  "initializationOptions": { "useGlobalConfig": false }
}
```

The server supports the `dprint/*` methods when the `capabilities` of its result have the `dprint` experimental capability:

```json
{ "experimental": { "dprint": { "version": 1 } } }
```

When they don't (ex. an older version of dprint), shut the server down and fall back to [schema version 5](./editor-extension-development-schema-5.md). The version changes when the methods change in a way an editor extension needs to know about.

One server formats the files of a whole workspace. For each file, the server finds the configuration file in an ancestor directory, or uses the global configuration file when that's allowed. The configuration is read again for each request, so an edited configuration file is used for the next request without restarting the server. Specifying `dprint lsp --config <path>` uses that configuration file for every file instead.

With the `dprint` capability, the server doesn't sync documents, register the text document methods, or provide completions for configuration files (the configuration schema method provides what's needed for that).

### Lifecycle

- The server exits when the process of the `processId` of the `initialize` request exits, when the `exit` notification is received, or when its stdin is closed.
- Send `shutdown` and then `exit` to stop it, which stops the plugins.
- Keep the server running for as long as the editor uses it. It notices when configuration files change, so there's no need to start it again when one is created, changed, or deleted, or when the workspace folders change (send `workspace/didChangeWorkspaceFolders`).
- Starting it again forgets what the user decided for the session (ex. denying a download), which is what a restart command is for. Use `dprint/reset` instead when only the plugins need to start again (ex. after a plugin stopped responding or was changed).
- A notification that changes the server's state (ex. `workspace/didChangeConfiguration`) applies to the requests sent after it.

### Settings

The settings are in the `initializationOptions` of the `initialize` request and the `settings` of `workspace/didChangeConfiguration`, which may have them at the top level or within a `dprint` property. A setting that isn't provided keeps its value.

- `useGlobalConfig` (boolean) - Whether to format a file without a configuration file in an ancestor directory with the global configuration file. Defaults to `false`, or `true` when the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable is `1` or `true`. The `dprint/*` methods also take it for a document (ex. for a command that formats with the global configuration file).
- `ensureStableFormat` (boolean) - Whether to format again until the text doesn't change, which defaults to what the `DPRINT_EDITOR_STABLE_FORMAT` environment variable says.
- `verbose` (boolean) - Whether to log debug messages.

IMPORTANT: Only provide `useGlobalConfig` from the user's settings and not from settings in the project (ex. `.vscode/settings.json`). The process plugins of the global configuration file run without being approved because it's the user's own.

## Documents

Documents are identified by their uri (ex. `file:///home/user/project/file.ts`). The plugin that formats a document is chosen by its file path, so a document that isn't a file is formatted as one:

- An untitled document (ex. `untitled:Untitled-1`) is formatted as a file in the first workspace folder, or the home directory when there isn't one.
- A notebook cell is formatted as a file in the notebook's directory when the `notebookDocument` is provided, which is what the jupyter plugin does when `dprint fmt` formats a notebook. It's only formatted when the notebook would be.

The file is named after the document's language (ex. `Untitled.ts` or `code_block.py`), so provide the `language` of these documents. The file name is the first of the language's file extensions, its file names, and then the file extension dprint knows for the language id that the configuration file formats, or otherwise the first one. A request for any other kind of uri fails.

```ts
interface DocumentParams {
  textDocument: { uri: string };
  /** Needed for a document that isn't a file (ex. an untitled document or a notebook cell). */
  language?: DocumentLanguage;
  /** The notebook when the document is one of its cells. */
  notebookDocument?: { uri: string };
  /** Whether to use the global config file when there's no config file in an ancestor directory, which is in addition to the setting. */
  useGlobalConfig?: boolean;
}

interface DocumentLanguage {
  /** The editor's id of the language (ex. "typescript"). */
  id: string;
  /** The file extensions the editor associates with the language with the leading period (ex. ".ts"). */
  fileExtensions?: string[];
  /** The file names the editor associates with the language (ex. "Dockerfile"). */
  fileNames?: string[];
}
```

Positions are in UTF-16 code units like the language server protocol's default.

## Which documents dprint formats

An editor usually needs to know which documents an extension formats before formatting them (ex. to register as their formatter). Get each configuration file's plugins with `dprint/configInfo`:

- A document whose file extension or file name is one of a plugin's may be formatted with it. Use `dprint/fileInfo` to know for sure (ex. the configuration file's `includes` and `excludes`).
- When `formatsOtherFiles` is `true`, the configuration file formats other documents too (ex. with a plugin's `associations` or `shebangs`), which only `dprint/fileInfo` says.
- When `plugins` is `null`, they aren't known (ex. a download or process plugin needs to be approved and they weren't set up before). Don't claim the documents of the configuration file, so the editor's other formatters are still used, and show what needs to be approved instead (see "Approving").
- When the configuration file has an error, `plugins` are the ones it had before, so a typo doesn't change what the editor formats with it.

Get them again when the server sends `dprint/configChanged`, which is for a change the editor can't see (ex. a configuration file it extends changed, or what needed to be approved was approved outside the editor), and when the editor sees a configuration file change.

## Approving

Opening a project isn't the same as choosing to run what its configuration file says, so the server only uses a configuration file once the user approves what it needs:

- **Downloads** - A download sends a request to the domain it's from, which a configuration file could use to send information to a server of its choosing (ex. that the project was opened and from where). The server only downloads plugins and the configuration files that a configuration file extends from these trusted domains (and their subdomains) without the user approving it: `registry.npmjs.org`, `plugins.dprint.dev`, `github.com`, and `githubusercontent.com`. This includes each redirect, and reading a file on another machine (ex. a UNC path on Windows) is approved like a download. What's in dprint's cache doesn't need to be downloaded (ex. from running `dprint fmt` or `dprint config install`), and neither does a plugin that's a file (ex. in `node_modules`).
- **Process plugins** - They aren't sandboxed, so running them needs to be approved for the configuration file with each plugin's configuration, unless the user allows them (see the `editor.allowProcessPlugins` setting in [the language server docs](../website/src/lsp.md)). Running dprint with the configuration file (`dprint fmt`, `dprint check`, or `dprint config install`) approves them because the user ran it.

Nothing is formatted with the configuration file until then, not even by its Wasm plugins, because formatting only some files or formatting them with other plugins would format differently than the CLI. The results of `dprint/format`, `dprint/fileInfo`, and `dprint/configInfo` have what needs to be approved:

```ts
interface PendingApproval {
  /** Identifies exactly what's approved, which changes when it does (ex. the configuration of a process plugin). */
  id: string;
  kind: "download" | "processPlugins";
  /** The urls or paths to download, or the process plugins to run. */
  items: string[];
  /** Whether the user denied it, which means they shouldn't be asked unless they ask for it (ex. a command). */
  denied: boolean;
  /** The message to show the user when asking. */
  message: string;
}
```

Ask the user when they want to format with the configuration file (ex. a format on save or a command), and not when a project is opened, because the editor makes requests without the user doing anything. Then send what they decided with `dprint/approve`. A download can be allowed or denied, which is only for the session, and process plugins can also be always allowed or denied. Approving a download only allows what was shown, so the result may have something else to approve (ex. a configuration file that the downloaded one extends). Format the document again once nothing else needs to be approved.

## Methods

The server responds with an error to a request that can't be handled (ex. a document that isn't a file), which has its message.

### `dprint/format` (request)

Formats a document.

```ts
interface FormatParams extends DocumentParams {
  /** The text of the document. */
  text: string;
  /** The ranges to format, which is the whole text when not provided. */
  ranges?: Range[];
  /** The plugin configuration to use instead of what's in the configuration file. */
  overrideConfig?: { [key: string]: unknown };
}

type FormatResult =
  /** The edits of the text that was sent, which don't overlap and are in order. It's empty when the text is already formatted. */
  | { kind: "formatted"; edits: TextEdit[] }
  /** Why it wasn't formatted, where the message is for a command the user ran. */
  | { kind: "notFormatted"; reason: NotFormattedReason; message?: string; approval?: PendingApproval }
  /** A plugin couldn't format it (ex. a syntax error) or failed (ex. it panicked). */
  | { kind: "failed"; reason: "formatError" | "pluginError" | (string & {}); message: string };

type NotFormattedReason =
  /** There's no config file in an ancestor directory and there's no global config file. */
  | "noConfigFile"
  /** There's no config file in an ancestor directory and the global config file isn't used. */
  | "globalConfigNotAllowed"
  /** The config file doesn't format it (ex. its includes and excludes don't match it). */
  | "notMatched"
  /** No plugin in the config file formats it. */
  | "noPlugin"
  /** A plugin that formats it doesn't support formatting a range. */
  | "rangeFormattingNotSupported"
  /** The configuration of a plugin has problems, which are in the diagnostics of the config file. */
  | "configDiagnostics"
  /** The config file failed to resolve, which the server shows the user once until it resolves. */
  | "configError"
  /** Something the config file needs has to be approved for download (see the approval). */
  | "downloadRequired"
  /** The process plugins of the config file need to be approved (see the approval). */
  | "processPluginsNotAllowed"
  | (string & {});
```

Each range is formatted in order, and overlapping ranges are merged. A plugin that supports range formatting decides what to format around a range, and the range is expanded to whole lines for a plugin that doesn't say. The edits are of the text that was sent, and the document's line endings are kept.

Only show a `formatError` to the user when they ran a command, because a format on save of a file with a syntax error would show one each time. Show a `pluginError`. Treat an unknown reason like `pluginError` and an unknown not formatted reason like `notMatched`.

### `dprint/fileInfo` (request)

Gets how a document is formatted, which doesn't need its text.

```ts
interface FileInfoResult {
  configFile: ConfigFileInfo | null;
  canFormat: boolean;
  /** The plugins that format it in the order they do. */
  plugins: { name: string; version: string }[];
  reason?: NotFormattedReason;
  message?: string;
  approval?: PendingApproval;
}

interface ConfigFileInfo {
  uri: string;
  isGlobal: boolean;
}
```

An extensionless file's shebang is read from the file on disk.

### `dprint/configInfo` (request)

Gets the plugins of a configuration file.

```ts
interface ConfigParams {
  /** The configuration file or a document it's used for. */
  uri: string;
  useGlobalConfig?: boolean;
}

interface ConfigInfoResult {
  configFile: ConfigFileInfo | null;
  /** `null` when they aren't known (see "Which documents dprint formats"). */
  plugins: PluginInfo[] | null;
  formatsOtherFiles: boolean;
  reason?: NotFormattedReason;
  message?: string;
  approval?: PendingApproval;
}

interface PluginInfo {
  name: string;
  version: string;
  configKey: string;
  /** The file extensions it formats without the leading period. */
  fileExtensions: string[];
  /** The file names it formats regardless of their extension. */
  fileNames: string[];
}
```

Getting it sets up the plugins that are approved (ex. downloads them), so get it once a configuration file is found so the first format doesn't wait on that.

### `dprint/configSchema` (request)

Gets the JSON schema of a configuration file (`ConfigParams`), which has the schemas of its plugins, so the editor can provide completions and validation for it. A plugin's schema is only downloaded from a trusted domain.

```ts
interface ConfigSchemaResult {
  schema: object;
}
```

### `dprint/approve` (request)

Approves what was pending.

```ts
interface ApproveParams {
  id: string;
  /** Only remembered for the session unless it's "alwaysAllow" or "alwaysDeny", which a download doesn't have. */
  decision: "allow" | "alwaysAllow" | "deny" | "alwaysDeny";
}

interface ApproveResult {
  /** What else needs to be approved now, which is null when the configuration file is ready. */
  approval: PendingApproval | null;
}
```

Allowing it downloads and sets up what was allowed. It fails when nothing is pending with the id, which is when what needs to be approved changed (ex. the configuration file changed).

### `dprint/reset` (request)

Forgets the configuration files and stops the plugins, which are set up again when they're next used (ex. after a plugin stopped responding or was changed). Unlike starting a new server, what the user decided for the session is kept.

### `dprint/configChanged` (notification from the server)

```ts
interface ConfigChangedParams {
  uri: string;
}
```

See "Which documents dprint formats".

### Language server protocol methods

The server also uses these, which an editor extension should handle:

- `window/logMessage` - What the server logs, which isn't written to stderr. Stderr only has what isn't (ex. a crash).
- `window/showMessage` - An error to show the user (ex. a configuration file that failed to resolve), which is only sent once until it changes because the editor makes requests without the user doing anything.
- `window/workDoneProgress/create` and `$/progress` - What's being done that may take a while (ex. downloading a plugin), when the client supports it. Don't time out a request while there's progress because the first request for a configuration file may download and set up its plugins.
- `textDocument/publishDiagnostics` - The problems with a configuration file (ex. an unknown property), which replace the ones published before. A problem without a known position is at the start of the file.
- `$/cancelRequest` - Cancels a request (ex. a format the editor doesn't need anymore).
