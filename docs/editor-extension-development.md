# Developing an Editor Extension

An editor extension formats documents with the `dprint lsp` command, which is a language server. A language client formats documents with the text document methods of the language server protocol (see [the language server docs](../website/src/lsp.md)). An editor extension uses the `dprint/*` methods described here instead, which are only sent a document's text when formatting it, so the server doesn't keep the documents. They also say which documents dprint formats and let the editor extension ask the user what needs to be approved.

The `dprint/*` methods were introduced in dprint 0.65. Editor extensions for older versions used `dprint editor-service` (see [schema version 5](./editor-extension-development-schema-5.md)), which formats the files of a configuration file that the editor found.

## Starting the server

Run `dprint lsp` in the workspace folder and send the `initialize` request with the `dprint` experimental capability:

```json
{
  "processId": 1234,
  "capabilities": {
    "experimental": { "dprint": { "versions": [1] } },
    "window": { "workDoneProgress": true }
  },
  "workspaceFolders": [{ "uri": "file:///home/user/project", "name": "project" }],
  "initializationOptions": { "useGlobalConfig": false }
}
```

`versions` are the versions of the `dprint/*` methods the editor extension supports. The server uses the highest one that it also supports, which is the `version` in the `dprint` experimental capability of the `capabilities` of its result:

```json
{ "experimental": { "dprint": { "version": 1 } }, "positionEncoding": "utf-16" }
```

When its result doesn't have the capability, the server doesn't support any of the versions (ex. it's an older version of dprint) and handles the client like a language client. Check this before sending `initialized`, and when it's missing, send `shutdown` and `exit` without sending `initialized` (otherwise the server registers the text document methods) and fall back to [schema version 5](./editor-extension-development-schema-5.md) or a newer editor extension. The result's `serverInfo.version` is the version of dprint (ex. to log it or to tell apart the servers of different executables).

An editor whose extensions can't send custom requests or handle custom notifications (ex. Zed or Helix) uses `dprint lsp` as a language client instead (see [the language server docs](../website/src/lsp.md)).

One server formats the files of a whole workspace. For each file, the server finds the configuration file in an ancestor directory, or uses the global configuration file when that's allowed. The configuration is read again for each request, so an edited configuration file is used for the next request without restarting the server. Specifying `dprint lsp --config <path>` uses that configuration file for every file instead.

With the `dprint` capability, the server doesn't sync documents, register the text document methods, or provide completions for configuration files (the configuration schema method provides what's needed for that).

### Compatibility

The editor extension says the versions of the `dprint/*` methods it supports and the server responds with the one it uses, so a server can support an old version alongside a new one:

- `versions` is a non-empty list of positive integers. A version the server can't read (ex. `"2"`) is ignored, and a capability that isn't an object with `versions` or that doesn't have a version the server supports has the server handle the client like a language client and leave the capability out of its result.
- A version is added for a change that an editor extension needs to know about (ex. one that would break it). Fields may be added to results, and values to the string unions of results (ex. kinds and reasons), within a version, so ignore fields that aren't known and handle values that aren't known like the methods below say.
- Optional behavior is opted into with `features`, an object of the features each side supports (`{ [name: string]: true | object }`, where an object has options). The client sends the ones it supports and the server responds with the ones it uses, so a feature is only used when both have it. A feature that isn't known is ignored. There aren't any yet.
- A field or a value of params that changes what the server does (ex. a new `globalConfig` value, a new decision, or an `overrideConfig` key) is only added with a new version or a feature, and the client only sends it when the server responded with that version or feature, because the server rejects a value it doesn't know (`-32602`) and ignores a field it doesn't know. Likewise, a new kind of result that an older client would handle wrongly with its fallback is only returned when the client sent the feature for it.

### Finding the executable

Use the `dprint` executable the user configured, or otherwise the one on the path. An editor extension may also use the one a project installed with npm (ex. `node_modules/@dprint/<platform package>/dprint` found from the workspace folder and its ancestors) so that the project's version of dprint is used. That executable is from the repository, so running it is the same as running code from the repository. Only do this in an editor that has workspace trust (ex. VS Code's Restricted Mode) once the user trusts the workspace.

### Lifecycle

- The server exits when the process of the `processId` of the `initialize` request exits, when the `exit` notification is received, or when its stdin is closed.
- Send `shutdown` and then `exit` to stop it, which stops the plugins. Clear the diagnostics it published once it exits, because they aren't published again until a configuration file is used.
- Keep the server running for as long as the editor uses it. It reads a configuration file again for each request that uses it, so there's no need to start it again when one is created, changed, or deleted, or when the workspace folders change (send `workspace/didChangeWorkspaceFolders`).
- The server doesn't watch the workspace. Send `workspace/didChangeWatchedFiles` when a configuration file (`dprint.json`, `dprint.jsonc`, `.dprint.json`, `.dprint.jsonc`, or `dprint.json` or `dprint.jsonc` in a `.config` directory) is created, changed, or deleted, when the `configFile.uri` of a `dprint/configInfo` result outside the workspace folders changes (ex. a configuration file in an ancestor directory of a workspace folder), and when one of its `watchUris` changes (ex. a configuration file it extends). The server resolves a configuration file that it resolved before again to publish its diagnostics, and sends `dprint/configChanged` when what a configuration file formats may have changed. It doesn't resolve one that wasn't used yet because that would download and set up its plugins without the user doing anything. A configuration file in `node_modules` is ignored, so leave those out of the file watcher. The server doesn't register to be told about them for an editor extension.
- The server watches dprint's own files, which are outside the workspace and depend on the system: its settings file (ex. a domain added to `editor.trustedDomains`), its `editor-approvals.json` (ex. the user ran `dprint fmt` or allowed a process plugin in another editor window), and the global configuration file. When one changes, it resolves the configuration files that were used again and sends `dprint/configChanged` for the ones whose results changed.
- When the server exits unexpectedly, start it again for the next request and get `dprint/configInfo` again for each configuration file the editor extension knows about, because the new server doesn't know them (ex. the files they were resolved from) or publish their diagnostics until they're used.
- Starting it again forgets what the user decided for the session (ex. denying a download), which is what a restart command is for. Use `dprint/reset` instead when only the plugins need to start again (ex. after a plugin stopped responding or was changed).
- A notification that changes the server's state (ex. `workspace/didChangeConfiguration`) applies to the requests sent after it.

### Settings

The settings are in the `initializationOptions` of the `initialize` request and the `settings` of `workspace/didChangeConfiguration`, which may have them at the top level or within a `dprint` property. A setting that isn't provided keeps its value, and a setting that's `null` is set back to its default.

- `useGlobalConfig` (boolean) - Whether to format a file without a configuration file in an ancestor directory with the global configuration file. Defaults to `false`, or `true` when the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable is `1` or `true`. The `dprint/*` methods also take `globalConfig` for a document (ex. for a command that formats with the global configuration file, or a setting of a workspace folder).
- `ensureStableFormat` (boolean) - Whether to format again until the text doesn't change, which defaults to what the `DPRINT_EDITOR_STABLE_FORMAT` environment variable says.
- `verbose` (boolean) - Whether to log debug messages.

The process plugins of the global configuration file run without being approved because it's the user's own.

## Documents

Documents are identified by their uri (ex. `file:///home/user/project/file.ts`). The plugin that formats a document is chosen by its file path, so a document that isn't a file is formatted as one:

- An untitled document (ex. `untitled:Untitled-1`) is formatted as a file in the first workspace folder, or the home directory when there isn't one.
- A notebook cell is formatted as a file in the notebook's directory when the `notebookDocument` is provided, which is what the jupyter plugin does when `dprint fmt` formats a notebook. It's only formatted when the notebook would be.

The file is named after the document's language (ex. `Untitled.ts` or `code_block.py`), so provide the `language` of these documents, which is otherwise not formatted for the `languageRequired` reason. The file name is the first of the language's file extensions, its file names, and then the file extension dprint knows for the language id that the configuration file formats, or otherwise the first one. Any other kind of uri isn't formatted for the `unsupportedDocument` reason (ex. a `git:` document).

```ts
interface DocumentParams {
  textDocument: { uri: string };
  /** Needed for a document that isn't a file (ex. an untitled document or a notebook cell). */
  language?: DocumentLanguage;
  /** The notebook when the document is one of its cells. */
  notebookDocument?: { uri: string };
  /** When the global config file is used when there's no config file in an ancestor directory, which is what the setting says when not provided. */
  globalConfig?: "whenNoConfigFile" | "never";
  /** The token to report the progress of the request with (ex. downloading the plugins of the configuration file). */
  workDoneToken?: string | number;
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

Positions are in the result's `positionEncoding`, which is `utf-16` like the language server protocol's default. Compare the uris the server provides (ex. `configFile.uri`) to the editor's as file paths, because they may be encoded differently (ex. `file:///C:/` and `file:///c%3A/`).

## Which documents dprint formats

An editor usually needs to know which documents an extension formats before formatting them (ex. to register as their formatter). Get each configuration file's plugins with `dprint/configInfo`:

- `filePatterns` are globs of the documents its plugins may format, which are relative to the directory of the configuration file (`configFile.dirUri`). Register them as the documents the editor extension formats, and the cells of the notebooks they match.
- `otherFiles` are the other documents the configuration file formats, which a glob can't match (ex. extensionless files with a `shebangs` entry), and only `dprint/fileInfo` says whether one of them is formatted.
- The patterns are a superset of what's formatted, because the configuration file's `includes` and `excludes` aren't in them and a configuration file in a descendant directory may format the documents there instead. Before acting as a document's formatter (ex. when it's opened), get its `dprint/fileInfo` and stop claiming it when its status is `notMatched`, `noPlugin`, `noConfigFile`, or `globalConfigNotAllowed`, or `approvalRequired` with only denied approvals, so the editor's other formatters are used. Keep claiming it for `configError`, `configDiagnostics`, and approvals that aren't denied, so the user is told why it isn't formatted.
- When something needs to be approved, `plugins` and `filePatterns` are what they were the last time the plugins were set up, which is remembered across sessions, so claim those documents and ask when the user formats one (see "Approving").
- When `plugins` is `null`, they aren't known (ex. a plugin that needs to be downloaded) and `filePatterns` is empty. Don't claim the documents of the configuration file, so the editor's other formatters are still used, and show what needs to be approved instead.
- When the configuration file has an error, `plugins` and `filePatterns` are the ones it had before, so a typo doesn't change what the editor formats with it.

Get them again only when the server sends `dprint/configChanged`, which it sends whenever what a configuration file formats may have changed: when one the editor said was created, changed, or deleted, when a file it was resolved from changed, when resolving it for a request has a different result (ex. a remote configuration file it extends changed), and when what needed to be approved was approved (including outside the editor, which the server notices in dprint's own files). The results are for a configuration file and the directory it's used for, so the global configuration file's are for each `dirUri` (ex. each drive).

## Approving

Opening a project isn't the same as choosing to run what its configuration file says, so the server only uses a configuration file once the user approves what it needs:

- **Downloads** - A download sends a request to the domain it's from, which a configuration file could use to send information to a server of its choosing (ex. that the project was opened and from where). To limit that, the server only downloads plugins and the configuration files that a configuration file extends from these trusted domains (and their subdomains) without the user approving it: `registry.npmjs.org`, `plugins.dprint.dev`, `github.com`, `githubusercontent.com`, and the ones the user added to the `editor.trustedDomains` setting in dprint's settings file. This includes each redirect, and reading a file on another machine (ex. a UNC path on Windows) is approved like a download. What's in dprint's cache doesn't need to be downloaded (ex. from running `dprint fmt` or `dprint config install`), and neither does a plugin that's a file (ex. in `node_modules`).
- **Process plugins** - They aren't sandboxed, so running them needs to be approved for the configuration file with each plugin's configuration, unless the user set `editor.allowProcessPlugins` to `true` in dprint's settings file (see [the language server docs](../website/src/lsp.md)). Running dprint with the configuration file (`dprint fmt`, `dprint check`, or `dprint config install`) approves them because the user ran it.

Nothing is formatted with the configuration file until then, not even by its Wasm plugins, because formatting only some files or formatting them with other plugins would format differently than the CLI. The results of `dprint/format`, `dprint/fileInfo`, and `dprint/configInfo` then have the `approvalRequired` reason and what needs to be approved:

```ts
interface PendingApproval {
  /** Identifies exactly what's approved, which changes when it does (ex. the configuration of a process plugin). */
  id: string;
  kind: "download" | "processPlugins" | (string & {});
  /** The configuration file that needs it, which is what the user approves for. */
  configFile?: string;
  /** What's downloaded or run. */
  items: ApprovalItem[];
  /** The decisions the user can make, in the order to show them. */
  decisions: { id: ApprovalDecision; label: string }[];
  /** Whether the user denied it, which means they shouldn't be asked unless they ask for it (ex. a command). */
  denied: boolean;
  /** What the user decided before, which is "deny" or "alwaysDeny" when it's denied. */
  previousDecision?: ApprovalDecision;
  /** The message to show the user when asking. */
  message: string;
}

interface ApprovalItem {
  /** The url, path, or npm specifier (ex. of a plugin with its checksum). */
  specifier: string;
  /** A config file is one that a config file extends. */
  kind?: "plugin" | "config" | (string & {});
  /** The name, version, and config key of a plugin that was set up before. */
  name?: string;
  version?: string;
  configKey?: string;
  /** The configuration a process plugin that was set up before runs with (ex. the commands of dprint-plugin-exec). */
  configuration?: unknown;
}

/** Only remembered for the session unless it's "alwaysAllow" or "alwaysDeny". Show a decision that isn't known by its `label`. */
type ApprovalDecision = "allow" | "alwaysAllow" | "deny" | "alwaysDeny" | (string & {});
```

Several may need to be approved at once (ex. downloading a plugin and running a process plugin), so they can be shown together. Ask about the approvals of a result when the user wants to format with the configuration file (ex. a format on save or a command), and not when a project is opened, because the editor makes requests without the user doing anything. Ask the same about a kind that isn't known, and only offer the approval's `decisions`. Show the configuration file (a missing `configFile` is one that isn't a file) and what will run (each item's `configuration`) so the user knows what they're approving. Also show that the configuration file needs approval somewhere that doesn't interrupt the user (ex. a status bar item that asks when it's clicked), because a denied approval isn't asked about again and the editor may not format a document with it. `dprint/pendingApprovals` has what needs to be approved for that.

The editor formats without the user doing anything in between (ex. a "save all" of many documents), so don't let asking turn into a storm of prompts:

- Show one prompt for an approval `id` at a time, which every format waiting on it shares.
- A format the user didn't run as a command asks about an `id` at most once per server session, even when the prompt is closed without deciding, because that leaves it pending. A command or the status bar item asks again, including about what was denied.
- Don't make a save wait on the prompt. Once it's approved, format the document's current text again, which may have changed in the meantime.

Then send what they decided with `dprint/approve`, one at a time, and take what to ask about next from its result, which is what needs to be approved now (ex. a configuration file that a downloaded one extends or the process plugins of a downloaded configuration file). Stop when the result is empty or only has denied approvals, and then format the document again. After `-32001` for a decision to deny, don't ask again right away. The "Always" decisions are stored in an `editor-approvals.json` file next to dprint's settings file, and deleting it has the user asked again, which the server notices (see "Lifecycle").

## Methods

The server responds with an error to a request that can't be handled, which has its message and a code:

- `-32602` (InvalidParams) - The params can't be handled (ex. an `overrideConfig` property that can't be overridden or a decision the approval doesn't have).
- `-32001` - What `dprint/approve` was sent for isn't pending anymore because what needs to be approved changed (ex. the configuration file changed). The error's `data` is `{ kind: "approvalChanged", approvals?: PendingApproval[] }`, which has what needs to be approved now when it's known, which is what to ask about. Otherwise get what's pending again (ex. with `dprint/pendingApprovals`).
- `-32800` (RequestCancelled) - The request was cancelled with `$/cancelRequest`, which isn't shown to the user.
- `-32601` (MethodNotFound) - The method isn't known (ex. one that's newer than the server).
- `-32603` (InternalError) - Anything else (ex. the server stopped). Treat a code that isn't known like this.

The `data` of an error is an object with a `kind`, where a kind that isn't known is ignored. The methods that take params take an object that may have fields added later, so a method without params yet takes `{}`, and every result is an object.

### `dprint/format` (request)

Formats a document.

```ts
interface FormatParams extends DocumentParams {
  /** The text of the document. */
  text: string;
  /** The ranges to format, which is the whole text when not provided and nothing when empty. */
  ranges?: Range[];
  /**
   * The editor's indentation settings for the document, which are used instead of what's in the configuration file.
   * Only send it when the user wants the editor's indentation to win (ex. an extension setting), because the result
   * otherwise differs from `dprint fmt`. Other configuration can't be overridden because it could have a plugin do
   * something the user didn't approve.
   */
  overrideConfig?: { indentWidth?: number; useTabs?: boolean };
}

type FormatResult =
  /** The edits of the text that was sent, which don't overlap and are in order. It's empty when the text is already formatted. */
  | { kind: "formatted"; edits: TextEdit[] }
  | ({ kind: "notFormatted" } & NotFormatted)
  /** A plugin couldn't format it (ex. a syntax error) or failed (ex. it panicked), which says the plugin when it's known. */
  | { kind: "failed"; reason: "formatError" | "pluginError" | (string & {}); message: string; plugin?: FilePlugin };

/** Why a document isn't formatted, which is the same for each method. */
interface NotFormatted {
  reason: NotFormattedReason;
  /** The message to show the user (ex. for a command the user ran). */
  message?: string;
  /** What needs to be approved to format it (see "Approving"). */
  approvals?: PendingApproval[];
}

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
  /** The config file failed to resolve. */
  | "configError"
  /** Something needs to be approved before the config file is used, which the approvals have. */
  | "approvalRequired"
  /** It can't be formatted (ex. its uri isn't a file, an untitled document, or a notebook cell). */
  | "unsupportedDocument"
  /** It isn't a file, so its language is needed to format it (see "Documents"). */
  | "languageRequired"
  | (string & {});
```

Each range is formatted in order, and overlapping ranges are merged. A plugin that supports range formatting decides what to format around a range, and the range is expanded to whole lines for a plugin that doesn't say. The edits are of the text that was sent, and the document's line endings are kept.

Only show a `formatError` to the user when they ran a command, because a format on save of a file with a syntax error would show one each time. Show a `pluginError`. Show the message of `configError` and `configDiagnostics` when the user formats a document (ex. a format on save or a command), once for each configuration file until the message changes or the server sends `dprint/configChanged` for it, and not for a request the editor made without the user (ex. `dprint/configInfo` when a project is opened). The server doesn't show them to an editor extension's user.

Treat a kind that isn't known like `failed` with `pluginError`, an unknown failed reason like `pluginError`, and an unknown not formatted reason like `notMatched`, showing its message when the user ran a command.

### `dprint/fileInfo` (request)

Gets how a document is formatted.

```ts
interface FileInfoParams extends DocumentParams {
  /** The text of the document, which is otherwise read from the file when it's needed (ex. the shebang of an extensionless file). */
  text?: string;
}

interface FileInfoResult {
  configFile: ConfigFileInfo | null;
  /** The plugins that format it in the order they do, which is empty when it isn't formatted. */
  plugins: FilePlugin[];
  status: FormatStatus;
}

/** Treat a kind that isn't known like "notFormatted" with "notMatched". */
type FormatStatus = { kind: "ready" } | ({ kind: "notFormatted" } & NotFormatted);

interface FilePlugin {
  name: string;
  version: string;
  configKey: string;
}

interface ConfigFileInfo {
  /** A file, https, or npm uri. */
  uri: string;
  /**
   * The directory whose files it formats, which is the directory it's in unless that's a `.config` directory,
   * and the directory it's used for when it's the global configuration file (ex. the root of a drive). It's the
   * directory its patterns are relative to for a remote or npm configuration file.
   */
  dirUri: string;
  /** How it was chosen: found in an ancestor directory, the global configuration file, or the one the server was started with (`dprint lsp --config <path>`). */
  kind: "local" | "global" | "override" | (string & {});
}
```

### `dprint/configInfo` (request)

Gets the plugins of a configuration file.

```ts
/** Either the configuration file itself or a document it's used for (see `DocumentParams`), and not both. */
interface ConfigParams extends Partial<DocumentParams> {
  configFile?: { uri: string };
}

interface ConfigInfoResult {
  configFile: ConfigFileInfo | null;
  /** `null` when they aren't known (see "Which documents dprint formats"). */
  plugins: PluginInfo[] | null;
  /** The documents the plugins may format. */
  filePatterns: FilePattern[];
  /** The other documents it formats, which only `dprint/fileInfo` says (see "Which documents dprint formats"). */
  otherFiles: { kind: "shebang" | "associations" | (string & {}); values: string[] }[];
  /** The local files besides the configuration file that it was resolved from (ex. a configuration file it extends), which the editor watches too. */
  watchUris: string[];
  status: FormatStatus;
}

interface FilePattern {
  /** A glob relative to `configFile.dirUri`, which matches file extensions and names case insensitively. */
  glob: string;
  /** The config keys of the plugins that format the documents. */
  configKeys: string[];
}

interface PluginInfo {
  name: string;
  version: string;
  configKey: string;
  /** The file extensions it formats without the leading period. */
  fileExtensions: string[];
  /** The file names it formats regardless of their extension. */
  fileNames: string[];
  /** Whether it formats a file in addition to the plugin that formats it instead of on its own (ex. sorting the keys of a package.json). */
  additive: boolean;
  /** Whether it decides what to format around a range, formats the whole lines of the range, or doesn't format a range. */
  rangeFormatting: "supported" | "wholeLines" | "unsupported" | (string & {});
  helpUrl: string;
  /** Where its newer version is said to be. */
  updateUrl?: string;
}
```

With `configFile`, it's the information of that configuration file even when its directory uses another one (ex. a `dprint.jsonc` beside a `dprint.json`), which needs the name of a configuration file or to be the global configuration file. It's the one the server was started with when there's one (`dprint lsp --config <path>`). Getting it sets up the plugins that are approved (ex. downloads them), so get it once a configuration file is found so the first format doesn't wait on that.

### `dprint/configSchema` (request)

Gets the JSON schema of a configuration file (`ConfigParams`), which has the schemas of its plugins, so the editor can provide completions and validation for it. A plugin's schema is only downloaded from a trusted domain.

```ts
interface ConfigSchemaResult {
  schema: object;
  /**
   * Whether it has the schema of each plugin. It doesn't when the configuration file can't be used (ex. its plugins need to be approved)
   * or a plugin's schema didn't download in time, in which case `dprint/configChanged` is sent once it did.
   */
  complete: boolean;
}
```

### `dprint/approve` (request)

Approves what was pending.

```ts
interface ApproveParams {
  id: string;
  /** One of the approval's decisions. */
  decision: ApprovalDecision;
  workDoneToken?: string | number;
}

interface ApproveResult {
  /**
   * What needs to be approved now for the configuration file, which is empty only when it's ready. It has what
   * was denied when the decision was to deny it, and otherwise it may have something else that needs to be approved.
   */
  approvals: PendingApproval[];
}
```

The decision is used once the server receives it, even when the request is cancelled. Allowing it downloads and sets up what was allowed, and then the configuration file is resolved again for the result. It fails with `-32001` when nothing is pending with the id.

### `dprint/pendingApprovals` (request)

Gets what needs to be approved for the configuration files that were used, which includes what the user denied (ex. for a status bar item or a command that asks about them). It takes `{}`.

```ts
interface PendingApprovalsResult {
  approvals: PendingApproval[];
}
```

### `dprint/reset` (request)

Forgets the configuration files and stops the plugins, which are set up again when they're next used (ex. after a plugin stopped responding or was changed). Unlike starting a new server, what the user decided for the session is kept. It takes `{}` and its result is `{}`. Get the plugins of the configuration files again afterwards because a plugin that changed may format other files.

### `dprint/configChanged` (notification from the server)

```ts
interface ConfigChangedParams {
  /** Every configuration file may have changed when it's missing (ex. dprint's settings file changed). */
  uri?: string;
  reason?: "created" | "changed" | "deleted" | (string & {});
  /** What changed, which is anything when it's missing. Get the schema again for "schema", but not the plugins. */
  changes?: ("schema" | (string & {}))[];
}
```

See "Which documents dprint formats". Get both the plugins and the schema again for a change that isn't known.

### Language server protocol methods

The server also uses these, which an editor extension should handle:

- `window/logMessage` - What the server logs, which isn't written to stderr. Stderr only has what isn't (ex. a crash).
- `window/showMessage` - Something to show the user that isn't in the result of a request (ex. failing to store an approval).
- `window/workDoneProgress/create` and `$/progress` - What's being done that may take a while (ex. downloading a plugin), when the client supports it. What's done while a request with a `workDoneToken` is handled is also reported on its token, which is the total of everything that's being done (ex. a download that started before the request). Don't time out a request while there's progress because the first request for a configuration file may download and set up its plugins.
- `textDocument/publishDiagnostics` - The problems with a configuration file (ex. an unknown property), which replace the ones published before. A problem without a known position is at the start of the file.
- `$/cancelRequest` - Cancels a request (ex. a format the editor doesn't need anymore).
- `workspace/didChangeWatchedFiles` - See "Lifecycle".

<!--
## Design decisions

These are kept here for the maintainers and aren't part of the protocol.

- `dprint/approve` takes one decision instead of a batch. Approvals depend on each other: allowing a download is what reveals what needs to be approved next (ex. a configuration file the downloaded one extends or the process plugins of a downloaded configuration file), so applying several decisions atomically doesn't fit. The client sends one at a time and takes the next ids from each result.
- The server watches dprint's own files (its settings file, `editor-approvals.json`, and the global configuration file) instead of asking the editor to, because their location depends on the system and every editor extension would otherwise need to find and watch them. It polls them because they're small and few. The workspace is still watched by the editor, which already watches it.
- A config file is resolved again when the editor says it changed only when it was resolved before, because resolving it may download and set up plugins without the user doing anything.
-->
