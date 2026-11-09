# Developing an Editor Extension

An editor extension formats documents with `dprint lsp`, a language server. Rather than the text document methods a language client uses (see [the language server docs](../website/src/lsp.md)), an extension uses the `dprint/*` methods described here: the server is only sent a document's text when formatting it, says which documents dprint formats, and lets the extension ask the user to approve what a configuration file needs. An editor whose extensions can't send custom requests or handle custom notifications (ex. Zed or Helix) uses `dprint lsp` as a language client instead.

The `dprint/*` methods were added in dprint 0.65. Older versions are used with `dprint editor-service` (see [schema version 5](./editor-extension-development-schema-5.md)).

An extension usually:

1. Starts `dprint lsp`, sends `initialize` with the `dprint` capability and its own `sessionId`, checks the server's `version`, and sends `initialized` (see "Starting the server").
2. Watches the configuration files (`configFileGlobs`) and the files they were resolved from (`watchUris`), and sends `workspace/didChangeWatchedFiles` when they change (see "Lifecycle").
3. Gets `dprint/fileInfo` when a document is opened, which says whether dprint formats it (`status`), whether to act as its formatter anyway (`claim`), and its `configFile`, and gets `dprint/configInfo` for that configuration file to know which other documents it may format (see "Which documents dprint formats").
4. Formats with `dprint/format`. For the `approvalRequired` reason, asks the user and sends the decision with `dprint/approve` (see "Approving").
5. Gets the `dprint/configInfo` and `dprint/fileInfo` results again on `dprint/configChanged`.
6. Shows each result's `message` as its `show` and `severity` say (see `dprint/format`).
7. Starts the server again with the same `sessionId` when it exits unexpectedly (see "Lifecycle").

## Starting the server

Start one `dprint lsp` for each `dprint` executable the editor uses (usually one per editor window, not per workspace folder), with the first workspace folder as its current directory (the home directory without one). Send `initialize` with the `dprint` experimental capability, then `initialized`, which is when the server registers what it needs from the client (ex. with the `watchFiles` feature):

```json
{
  "processId": 1234,
  "capabilities": {
    "experimental": { "dprint": { "versions": [1], "sessionId": "0f8c2b1e-5a4d-4c3b-9e2f-7a6b5c4d3e2f" } },
    "window": { "workDoneProgress": true }
  },
  "workspaceFolders": [{ "uri": "file:///home/user/project", "name": "project" }],
  "initializationOptions": { "useGlobalConfig": false }
}
```

The server uses the highest of the client's `versions` that it supports, which is the `version` of the `dprint` capability in its result:

```json
{
  "experimental": {
    "dprint": {
      "version": 1,
      "versions": [1],
      "files": {
        "settingsUri": "file:///home/user/.config/dprint/settings.json",
        "approvalsUri": "file:///home/user/.config/dprint/editor-approvals.json",
        "globalConfigUri": "file:///home/user/.config/dprint/dprint.jsonc",
        "globalConfigFileNames": ["dprint.jsonc", "dprint.json"]
      },
      "configFileGlobs": ["**/dprint.json", "**/dprint.jsonc", "**/.dprint.json", "**/.dprint.jsonc"],
      "configFileNames": ["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"],
      "dotConfigFileNames": ["dprint.json", "dprint.jsonc"]
    }
  },
  "positionEncoding": "utf-16"
}
```

Only use the server when its capability has a `version`:

- **No `version`:** the server supports none of the extension's versions. It advertises nothing else and every method fails with `-32601`. Show the capability's `message` (it says whether dprint or the extension needs updating) and stop it with `shutdown` and `exit`. Don't fall back to schema version 5, which that dprint may not have.
- **No `dprint` capability:** dprint is older than 0.65 and handles the client like a language client, so stop it right away and fall back to [schema version 5](./editor-extension-development-schema-5.md) or tell the user to update. dprint older than 0.45 has no `lsp` and exits before responding, which is handled the same way rather than as a failed start; detect it by running `dprint --version` first, not by parsing stderr.

`serverInfo.version` is the dprint version (ex. for logging). With the `dprint` capability, the server doesn't sync documents, register the text document methods, or provide completions for configuration files (use `dprint/configSchema`).

```ts
interface ClientCapability {
  /** The versions of the `dprint/*` methods the extension supports (positive integers). */
  versions: number[];
  /** See "Features". */
  features?: { [name: string]: true | object };
  /** See "Session id". */
  sessionId?: string;
}

interface ServerCapability {
  /** The version the server uses, missing when it supports none of the client's. */
  version?: number;
  /** The versions the server supports. */
  versions: number[];
  /** The client features it uses, missing when none (see "Features"). */
  features?: { [name: string]: true | object };
  /** The rest are missing without a `version`. */
  files?: { settingsUri?: string; approvalsUri?: string; globalConfigUri?: string; globalConfigFileNames?: string[] };
  configFileGlobs?: string[];
  configFileNames?: string[];
  dotConfigFileNames?: string[];
  /** What to tell the user when there isn't a `version`. */
  message?: string;
}
```

- `files` - dprint's own files, for commands that open them (ex. to add a domain to `editor.trustedDomains` in the settings file). A file is left out when its directory can't be found. `globalConfigUri` is where `dprint init --global` would create the global configuration file. These are from when the server started, so a command that opens the global configuration file opens the `uri` of a `configFile` whose `kind` is `global` when it has one, and otherwise checks for the `globalConfigFileNames` in order in `globalConfigUri`'s directory (`dprint.jsonc` then `dprint.json` when that field is missing) before offering to create it. Don't edit the approvals file; use `dprint/forgetDecisions`.
- `configFileGlobs` - The configuration files to watch in the workspace folders (see "Lifecycle").
- `configFileNames` and `dotConfigFileNames` - The names looked up in a directory and in its `.config` directory (not the home directory's). They're found in any case on a case insensitive file system (Windows and macOS by default). A newer version may add names, so prefer these over names of your own.

All three are empty for `dprint lsp --config <path>`, which uses that one configuration file for every file, so `dprint/configInfo` fails with `-32602` for any other. The path is a local path, a `~/` path, or a `file:` url; the server exits at start for a remote url or `npm:` specifier, so extend those from a local configuration file.

Otherwise one server formats every file, including ones outside the workspace folders, with the configuration file in an ancestor directory, or the global configuration file when allowed. The workspace folders (or `rootUri`) decide where untitled documents are formatted and whether a configuration file is `inWorkspace`. When no `dirUri` is provided, the global configuration file is used for the root of the server's current directory (ex. its drive).

### Session id

The server keeps the session's decisions (ex. a denied download, or a process plugin allowed for the session) in a file named by `sessionId` in dprint's cache directory, so a server started again with the same id after a crash keeps them without the extension sending anything again.

- Make a random one (ex. a UUID) in the extension for each server it starts, never from the project (ex. workspace settings). It's 16 to 128 of `A-Z`, `a-z`, `0-9`, `_`, and `-`. Randomness matters because ids that differ only in case are the same file on a case insensitive file system. An invalid id is ignored (and logged); without one, the session's decisions are lost when the server exits.
- Reuse it only to start the same server again after its process exited without the session ending (ex. it crashed, or the extension killed it). After `shutdown` and `exit`, or for the restart command, use a new one.
- Wait for the previous process to exit first. Until it does (or while the session's lock can't be taken, ex. without lock support), the new server doesn't read or write the session file, and `dprint/approve` and `dprint/forgetDecisions` results have a `notice` that its decisions won't be kept. If it exits before taking the file over, the next server with that id gets the other server's decisions (ex. an allow the user denied in this one).
- An exit with code 0 and no signal only happens after `shutdown`, so the session ended (ex. nvim-lspconfig's `:LspRestart`): use a new id and forget which approvals were asked about. For any other exit, decide by whether the extension stopped the server itself (ex. a flag set before it kills it or calls a library's restart), not by the exit code: a killed server exits with code 1 on Windows, and runtimes report signals differently (libuv's `on_exit` gives code 0 and the signal, Python a negative code, Java 128 + the signal, Node a `null` code with the signal). When only `on_exit` is known, treat an exit with a signal like one the client caused.
- A library that starts a server per project (ex. eglot or lsp-mode) needs an id per project root (ex. keyed by Neovim's `root_dir`), and session decisions and plugins aren't shared between those servers. Reuse the id when the library restarts a crashed server (ex. eglot's `eglot-autoreconnect`, or a `before_init` lookup), and clear it in the library's restart hook or the extension's restart command, because a library's restart may stop the server without `shutdown` (ex. Neovim's `Client:stop(true)`), which a `before_init` lookup can't tell from a crash.
- A new id is the only way to start a session over. `dprint/forgetDecisions` without params instead forgets every stored decision of every configuration file and keeps the session's allowed downloads.

### Failing to start

A server that exits before it responds to `initialize` failed to start. Show the user its stderr and don't start it again for each request. Start it again only when the settings the extension sends to dprint change (ex. arguments or environment), the `dprint` executable changes, the user runs the restart command, or the extension starts its servers again for another reason (ex. the workspace folders changed). A configuration file change doesn't fix it, because the server doesn't read one before responding. This doesn't apply to a server the extension stopped itself, or to dprint older than 0.45 (see above).

### Security

dprint never contacts another machine (ex. a share that a symlink in a repository points to, which on Windows is sent the user's credentials) before the user allows it, for anything done without the user asking: reading, `stat`, canonicalizing, watching, and looking for configuration files. That covers the server and what the extension does on its own (ex. watching, or getting the results of open documents), which follows the rules in this document. How a language client library handles `watchFiles` registrations and diagnostic uris is its own, like the editor's workspace search. When the user opens a file themselves (ex. by clicking a diagnostic), it's only a best effort: the server takes back stale diagnostics, and the extension checks `ConfigFileInfo.onAnotherMachine` before opening a configuration file.

The extension decides what the server is started with, so a project can't:

- Use the `dprint` executable the user configured, or else find the one on the path yourself in the path's absolute directories and start it by absolute path. A bare `dprint` may run one from the current directory, which is the workspace folder (ex. Node's `child_process` and Neovim on Windows). Start the executable itself, not a shell, batch file, or wrapper (ex. npm's `dprint.cmd`, or the `bin.cjs` Node wrapper), because killing the wrapper leaves the server running and holding its session. Resolve the wrapper to its executable (ex. the `dprint(.exe)` in the `dprint` package), or kill the whole process tree (ex. `taskkill /T /F` on Windows, the process group elsewhere) and wait for the server to exit.
- A project's npm-installed dprint (ex. `node_modules/@dprint/<platform package>/dprint` in the workspace folder or an ancestor) is code from the repository, so only use it in an editor with workspace trust (ex. VS Code's Restricted Mode) once the user trusts the workspace.
- Take the executable, arguments, and environment only from the user's or machine's settings, or the workspace's once it's trusted. Otherwise a project could point `DPRINT_CONFIG_DIR`, `XDG_CONFIG_HOME`, `HOME`, or `APPDATA` at a settings file that allows process plugins, an approvals file, or a global configuration file (whose process plugins run without asking), point `DPRINT_CACHE_DIR` at its own plugins or session files, or set a proxy (ex. `HTTPS_PROXY` or `NPM_CONFIG_HTTPS_PROXY`) that receives every download. The editor's own environment may come from the project (ex. direnv).
- Make the `sessionId` in the extension (see "Session id").
- When asking about a configuration file, show where it is, and that it's outside the workspace when `inWorkspace` is `false`, because one in an ancestor directory (ex. `/tmp`) may not be the project's.
- Show every string the server provides as plain text, because it may have text from a configuration file. Rendering Markdown or HTML could show a project's link that runs an editor command (ex. VS Code's `[label](command:...)` links), so escape it (ex. break up the `](`). The server escapes project text in an approval's `message` and its items' `specifier` and `registry` (see "Escaping"). Escape the same way what you show from a uri (ex. a `configFile`, `npmrcUri`, or `localUri` path, which may have a newline or bidi override) and from an approval's data, which the server doesn't escape: items' `configuration` and `plugin`, and `trustedConfiguration` (ex. a bidi override would disguise a dprint-plugin-exec command). Escaping the JSON text works. Skip this only when the editor shows those characters visibly.

### Compatibility

- `versions` is a non-empty list of positive integers; values the server can't read (ex. `"2"`) are ignored. A version is added for a change that would break an extension.
- Within a version, fields may be added to results and to the params of what the server sends, and values to string unions (ex. kinds and reasons). Ignore unknown fields and handle unknown values as the types below say. Prefer the fields the server provides for that (ex. `claim`, `show`, `severity`, `allows`) over deciding from a value.
- A new param field or value that changes what the server does (ex. a `globalConfig` value or `overrideConfig` key), and a new method or notification to the server, only come with a new version or feature, so only send them once the server responded with it. The server rejects unknown values (`-32602`), ignores unknown fields and notifications, and answers unknown requests with `-32601`. Decisions are the exception: only send one of an approval's `decisions`.
- A new result kind that an older client's fallback would mishandle, a field that tells the client not to do something (ex. `onAnotherMachine`, or `schemaOnly`), and a new notification or request from the server (including LSP ones a library may answer by itself, ex. `workspace/configuration`) only come with a version or feature the client sent. Without them, the server leaves out what an older client must not use (ex. the uri). `window/showMessage`, `window/logMessage`, `$/progress`, and `textDocument/publishDiagnostics` may be sent in any version. Ignore unknown notifications and answer unknown requests with `-32601`.
- Unknown settings or settings of the wrong type are ignored, so settings may be sent to any server. `initializationOptions` are sent before the version is known, so send a setting value that needs a version or feature with `workspace/didChangeConfiguration` once the server responded with it.
- The glob syntax, error codes, and UTF-16 positions don't change within a version. Strings are plain text, not Markdown.
- Optional params may be `null`, the same as missing. Results provide every non-optional field (`null` only where typed so) and leave out optional fields instead of sending `null`.

### Features

Optional behavior is opted into with `features` (`{ [name]: true | object }`, where an object has options). The server responds with the ones it uses in its `features` (`true`, or an object of the options it uses), which is left out when it uses none. Only use a feature in the server's `features`, and only the options in its object. Unknown features are ignored, and a feature applies to whichever version is used.

```json
{ "experimental": { "dprint": { "versions": [1], "features": { "fileUri": true, "watchFiles": true } } } }
```

- `fileUri` - A document is formatted as the file of its `fileUri` instead of its uri (see "Documents").
- `watchFiles` - The server registers the files it needs to be told about (`workspace/didChangeWatchedFiles`) instead of the extension watching them, for an extension on a language client library that handles registration (ex. for Neovim, Emacs, or Sublime Text). It's only used when the client supports dynamic registration with relative patterns (`dynamicRegistration` and `relativePatternSupport`). The used configuration files and their `watchUris` are registered by canonical directory, so their changes are reported with canonical uris, which the server handles. Ancestor directories of the workspace folders aren't registered, so still handle a configuration file created there (see "Which documents dprint formats"). A library may watch each registered directory recursively (ex. Neovim, which on Linux polls with a `stat` that follows symlinks), so a configuration file in an ancestor directory (ex. a monorepo's root) makes it watch that whole tree, which may reach another machine through a symlink. That's accepted as the library's behavior; to avoid it, leave out `watchFiles` and watch the files yourself.

### Lifecycle

Stopping:

- The server exits when the `processId` process exits, on `exit`, or when its stdin closes.
- `shutdown` then `exit` stops the plugins and ends the session (the session file is deleted). A language client library's stop sends both, so the next server needs a new `sessionId`.
- To stop a server and keep its session (ex. it stopped responding), kill its process (tree) or send `exit` without `shutdown`, and wait for it to exit (kill it forcibly if needed) before starting the next one with the same id. For a request that hangs in a plugin, try `dprint/reset` first. Session files of servers that crashed and weren't started again are deleted after a week.
- Clear the diagnostics a server published once it exits; they're only published again when a configuration file is used.

Running:

- Keep the server running while the editor uses it. It reads a configuration file again for each request, so don't restart it when one changes or when the workspace folders change (send `workspace/didChangeWorkspaceFolders`).
- A notification that changes the server's state (ex. `workspace/didChangeConfiguration`) applies to the requests sent after it.
- The server watches dprint's own files: its settings file, `editor-approvals.json` (ex. the user ran `dprint fmt` or decided in another window), and the global configuration file. When one changes, it sends `dprint/configChanged` for the used configuration files whose results changed.
- It doesn't watch dprint's cache, so a plugin downloaded outside the editor (ex. by `dprint config install`) is only used once the configuration file is resolved again (ex. after an approval or `dprint/reset`).
- It doesn't send `dprint/configChanged` for `.gitignore` changes, which change what a configuration file formats, so get `dprint/fileInfo` for the open documents again when the editor sees one change, if it watches them.

Watching the workspace (unless the server uses `watchFiles`):

- Send `workspace/didChangeWatchedFiles` when a configuration file is created, changed, or deleted (one of the `configFileGlobs` in the workspace folders, or of the `configFileNames` in their ancestor directories when you watch those, see "Which documents dprint formats"), when one of a `dprint/configInfo` result's `watchUris` changes (ex. a file it extends), and when its `configFile.uri` changes and is outside the workspace folders or of `kind: "cli"`. Not for `kind: "global"`, which the server watches itself.
- Changes to configuration files in `node_modules` are ignored, so the `configFileGlobs` watcher can leave it out, but still send the changes of `watchUris` there (ex. a shared configuration file a package provides).
- Don't follow symlinks, which may reach another machine the user didn't allow (ex. a `.config` directory a `git pull` replaced with a symlink to a share). This applies to every watcher, including of a configuration file whose `onAnotherMachine` is `true`:
  - Watch a file by watching its directory with a watcher that doesn't follow the symlinks in it (ex. VS Code's file watchers or inotify of the directory), not by watching the file or polling with a `stat` that follows symlinks (ex. Node's `fs.watchFile`).
  - Never watch a directory that's a symlink or reached through one, apart from a workspace folder or its ancestors. Each time a watcher starts (including after its directory was deleted and created again), `lstat` each directory of its path from the nearest workspace folder or ancestor down, because `lstat` follows a symlink before the last component (ex. `apps/web/shared/sub` after `apps/web/shared` became a symlink to a share).
  - Watch a file whose directory doesn't exist yet by watching the nearest existing directory of its path (checked the same way) for the next name, then each directory down as it's created. A watcher that starts once its directory was created reports each watched file already in it as created (ex. after a `git checkout` or `npm ci`). A file whose directory isn't watched is noticed by the next request that uses it.

Starting it again:

- When the server exits unexpectedly after it responded to `initialize`, start it again for the next request (with a growing delay when it keeps exiting) once its process exited, with the same `sessionId` unless it exited with code 0 without a signal (see "Session id"). Then get `dprint/configInfo` for each known configuration file and `dprint/fileInfo` for the open documents, because the new server doesn't know them. With the same id, it asks about the same approvals as the one that exited (except that process plugins are asked about again when the approvals file changed since). Get `dprint/pendingApprovals` for the status bar item once those results came back.
- Keep which approvals the user was asked about (see "Asking" for what makes two the same), including across restarts within a session, so a format on save doesn't ask again. That's the only decision state the extension keeps.
- A restart command forgets the session's decisions: stop the server with `shutdown` and `exit`, start one with a new `sessionId`, and forget which approvals were asked about. Use `dprint/reset` when only the plugins need to start again.
- When `dprint/forgetDecisions` responds, forget which approvals were asked about for its configuration file (for any `dirUri` when sent without one), or all of them when sent without one.
- Session files are in `editor-sessions` in dprint's cache directory, which every installed dprint shares and cleans up, so don't keep other files there or make it a symlink (its format and rules are documented on `SessionFile` in the source). `dprint clear-cache` stops every running `dprint lsp` and deletes the directory, so the extension starts its servers again (with the same `sessionId`, which starts empty).

### Which requests set up plugins

`dprint/format`, `dprint/fileInfo`, `dprint/configInfo`, `dprint/configSchema`, `dprint/approve`, `dprint/pendingApprovals`, and `dprint/forgetDecisions` resolve configuration files, which downloads and sets up their approved plugins, so they take a `workDoneToken`. Getting `dprint/fileInfo` when a document is opened (or `dprint/configInfo` for a configuration file the editor found) has the plugins ready by the time the user formats.

The first setup may take a while and is reported as progress. Don't time out a request while there's progress for it. A format on save with a deadline (ex. a synchronous one in Neovim) can cancel with `$/cancelRequest`, which doesn't stop the setup, so a later format uses the plugins.

### Settings

Settings are in the `initializationOptions` of `initialize` and the `settings` of `workspace/didChangeConfiguration`, at the top level or in a `dprint` object (which is then the only one used). A missing setting keeps its value, `null` resets it to its default, and an unknown one is ignored. Changing `useGlobalConfig` or the workspace folders sends `dprint/configChanged` without a `configFile`.

- `useGlobalConfig` (boolean) - Whether to format a file without a configuration file in an ancestor directory with the global configuration file. Defaults to `false`, or `true` when the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable is `1` or `true`. The `dprint/*` methods also take `globalConfig` for a document (ex. for a command or a workspace folder setting).
- `ensureStableFormat` (boolean) - Whether to format again until the text doesn't change. Defaults to the `DPRINT_EDITOR_STABLE_FORMAT` environment variable.
- `verbose` (boolean) - Whether to log debug messages.

## Documents

Documents are identified by their uri. The plugin that formats a document is chosen by its file path, so a document that isn't a file is formatted as one:

- An untitled document (any `untitled:` uri, ex. `untitled:<buffer number>` for an editor without its own) is formatted as a file in the first workspace folder, or the home directory. That's the first of `initialize`, and folders added later go after the others. To choose the directory (ex. the active document's workspace folder), use the `fileUri` feature with the exact file to format it as (ex. `file:///home/user/project/Untitled-1.ts`), whose name decides the plugin.
- A notebook cell is formatted as a file in the notebook's directory when `notebookDocument` is provided, like the jupyter plugin does with `dprint fmt`, and only when the notebook would be.
- With the `fileUri` feature, a document with a `fileUri` is formatted as that file (ex. a `git:` document, or a virtual file system backed by a file).
- Any other uri isn't formatted (`unsupportedDocument`).

An untitled document or notebook cell without a `fileUri` is named after its `language`, without which it isn't formatted (`languageRequired`). The name is the first of these that the configuration file formats, or else the first of them: each of the `fileExtensions`, each of the `fileNames`, then the extension dprint knows for the `id`. `language` isn't used with a `fileUri`.

dprint knows these language ids (VS Code's, compared case insensitively): `bash`, `sh`, `shell`, and `shellscript` (`.sh`), `c#` and `csharp` (`.cs`), `c++` and `cpp` (`.cpp`), `clojure` (`.clj`), `coffeescript` (`.coffee`), `elixir` (`.ex`), `erlang` (`.erl`), `f#` and `fsharp` (`.fs`), `handlebars` (`.hbs`), `haskell` (`.hs`), `javascript` (`.js`), `javascriptreact` (`.jsx`), `julia` (`.jl`), `kotlin` (`.kt`), `latex` (`.tex`), `markdown` (`.md`), `nushell` (`.nu`), `ocaml` (`.ml`), `perl` (`.pl`), `powershell` (`.ps1`), `proto3` and `protobuf` (`.proto`), `python` and `python3` (`.py`), `restructuredtext` (`.rst`), `ruby` (`.rb`), `rust` (`.rs`), `terraform` (`.tf`), `typescript` (`.ts`), `typescriptreact` (`.tsx`), and `yaml` (`.yml`). Another id of only letters and digits is used as the extension (ex. `toml`), and an id with other characters isn't known (ex. `yaml.ansible`). An editor with other ids (ex. a Neovim filetype or Emacs mode) provides `fileExtensions` or `fileNames`.

```ts
interface DocumentParams {
  textDocument: { uri: string };
  /** Needed for an untitled document or a notebook cell. */
  language?: DocumentLanguage;
  /** The notebook of a cell, with the file to format its cells as with the `fileUri` feature. */
  notebookDocument?: { uri: string; fileUri?: string };
  /** The file uri to format the document as, which is only used with the `fileUri` feature. */
  fileUri?: string;
  /** Whether to use the global config file when there's none in an ancestor directory, which defaults to the setting. */
  globalConfig?: "whenNoConfigFile" | "never";
  /** The token to report the request's progress with (ex. downloading plugins). */
  workDoneToken?: string | number;
}

interface DocumentLanguage {
  /** The editor's id of the language (ex. "typescript"). */
  id: string;
  /** With or without the leading period (ex. ".ts" or "ts"). */
  fileExtensions?: string[];
  /** Ex. "Dockerfile". */
  fileNames?: string[];
}
```

Positions are in UTF-16 code units (`positionEncoding`), whatever the client says it supports.

### Paths

Send the editor's own uris. The server works with canonical paths, but the uris it provides (ex. `configFile.uri` and `dirUri`, `watchUris`, diagnostics, and `dprint/configChanged`) are the paths the editor knows the files by (ex. through a symlink, a junction, a `subst` drive, or macOS's `/tmp`), learned from the workspace folders and the uris it's sent. Only the directories that `watchFiles` registers are canonical.

- A directory the editor knows by several paths is provided by the one it used last. `dprint/configChanged` only names that one, and its configuration file's diagnostics move there. Don't map paths to find what the editor has by the other path, because that follows symlinks; getting the results again on `dprint/configChanged` covers them.
- A configuration file that's a symlink (or in a `.config` directory that's one) is known by where it was found or asked about, and is used for that directory. For `~/proj/dprint.json -> ~/dotfiles/dprint.json`, its `uri` is `~/proj/dprint.json`, its `dirUri` is `~/proj`, and its canonical path is in `watchUris`. So two symlinks to the same file are two configuration files, and a symlink to the global configuration file isn't the global one. Every method that takes a `configFile` accepts any result's. On a case insensitive file system, a configuration file in another case on disk (ex. `.Config/dprint.json`) is known by the names dprint looks up (`.config/dprint.json`).
- `watchUris` are the editor's paths only through a workspace folder or its ancestors, and otherwise canonical, because the editor doesn't watch through other symlinks. A symlinked configuration file whose target was deleted keeps the target in `watchUris`, so its creation is reported.
- Once a directory the editor knows by another path doesn't lead where it did, the server provides another of the editor's paths that still leads there, or the canonical path, which then identifies a different configuration file.

Uris are encoded like the `url` crate does (ex. `file:///C:/Users/user/project`), which may differ from the editor's (ex. `file:///c%3A/Users/user/project`), so compare them as file paths, case insensitively on a case insensitive file system, and match the `configFileGlobs` case insensitively there too.

## Which documents dprint formats

The `configFile` of a document's `dprint/fileInfo` is the configuration file used for it, which may be in an ancestor directory of the workspace folder. To know them before a document is opened (ex. to register as a formatter, or to decide which servers to start), the extension may search for them itself:

- Search the workspace folders with the `configFileGlobs`, and the ancestor directories of the workspace folders with the `configFileNames` (and the `dotConfigFileNames` in each one's `.config` directory, other than the home directory's). Use the server's names (start a server first where possible), falling back to names you know. A name in a `.config` directory that isn't one of the `dotConfigFileNames` isn't a configuration file (`-32602`).
- Leave out what the editor excludes from searches (ex. build directories) and `node_modules`, because getting `dprint/configInfo` sets up a configuration file's plugins, which may download plugins no document uses.
- Don't follow symlinks: check with `lstat` rather than `stat`, don't look in a `.config` directory that's a symlink, and stop at the root of a network share or automounter directory like the server does (ex. don't look up `/net/dprint.json`, which the automounter would treat as a host). The search of the ancestor directories never follows a symlink. The workspace search doesn't follow directory symlinks where the editor's API allows that (ex. `os.walk(followlinks=False)`, or `fd` and ripgrep by default); when it doesn't (ex. VS Code's `findFiles`), say so in a code comment.

`dprint/configInfo` has what a configuration file may format:

- `filePatterns` - Globs relative to `configFile.dirUri`. Register them (and the cells of notebooks they match) as the documents the extension formats, unless the status's `claim` is `false`.
- `otherFiles` - What a glob can't match:
  - `shebangs` - Extensionless files whose first line is one of the values, or starts with one followed by whitespace (ex. `#!/usr/bin/env node`). Register extensionless files (ex. by checking the first line, or all of them) and get their `dprint/fileInfo`.
  - `associations` - A plugin's `associations` patterns that can't be made relative to `configFile.dirUri` (ex. `../shared/**/*.ts`) or have too many brace alternatives. The values are for showing the user, not globs to match, so treat every document like it may be formatted and get its `dprint/fileInfo`.
  - Treat an unknown kind like every document may be formatted.
- The patterns are a superset, because `includes` and `excludes` aren't in them and a descendant configuration file may format those documents instead. Before acting as a document's formatter (ex. when it's opened), get its `dprint/fileInfo`, and only act as its formatter when its status is `ready` or its `claim` is `true`, so the editor's other formatters are used otherwise. An editor without a formatter registry (ex. one that runs `dprint/format` on every save) likewise only formats claimed documents. The server claims a document that isn't formatted when the user needs to be told why (ex. a configuration error, or an approval that isn't denied).
- When something needs to be approved, `plugins` and `filePatterns` are the ones from the last time the plugins were set up (remembered across sessions). Claim those documents and ask when the user formats one (see "Approving"). Only those are `approvalRequired`; others are `notMatched` or `noPlugin`.
- When `plugins` is `null` (ex. a plugin needs to be downloaded first), `filePatterns` is empty, and every document in the directory is `approvalRequired` with `claim` set to `false`. Don't claim them, and show what needs to be approved instead.
- When the configuration file has an error, `plugins` and `filePatterns` are the previous ones, so a typo doesn't change what the editor formats. Only those documents are `configError` with `claim` set to `true`. When they aren't known (`plugins` is `null`), nothing is claimed.

The globs are VS Code's and use `/`, without a leading `/` or `./`. A `**` segment matches any number of segments (including none), `*` any characters within a segment, and `?` one character, including in names starting with a period (ex. `**/*.json` matches `.config/.eslintrc.json`, like picomatch's `dot: true`). `[...]` matches one character of a class, which may have ranges (ex. `[a-z]`) and be negated with a leading `!`, and never matches `/`. There are no braces or negated patterns. A literal special character (`*`, `?`, `[`, `{`, or `}`) is in a class of its own (ex. `[*]`), and extensions and names are matched case insensitively with classes (ex. `**/*.[tT][sS]`). Check another glob library's options (ex. picomatch and minimatch skip dot files by default, and Python's `fnmatch` has `*` match `/`).

The results stay valid until one of these, when they're gotten again:

- `dprint/configChanged` for the configuration file or without a `configFile`, which is sent whenever what a configuration file formats may have changed (ex. a file it was resolved from changed, something was approved or denied, including outside the editor, or it's on another machine now). It's sent without a `configFile` when `useGlobalConfig`, the workspace folders, or the existence of the global configuration file changed.
- `dprint/approve`, `dprint/forgetDecisions`, or `dprint/reset` responded. A `dprint/configChanged` may come for the same change, so getting the results once for both is enough (ex. by waiting a moment).
- The server was started again.

Then, except for a `schemaOnly` `dprint/configChanged` (which only needs the schema), get `dprint/configInfo` for each configuration file the extension knows (and the one named, if unknown, only when its `change` is `created`), and `dprint/fileInfo` for every open document, because `includes` and `excludes` may have changed. That also covers documents the editor knows by another path (see "Paths") and documents formatted as another file (ex. untitled ones).

A configuration file created in an ancestor directory outside the workspace folders isn't reported by those watchers, so either watch the `configFileNames` in the ancestor directories of the workspace folders (non-recursively, stopping where the server stops looking), plus the `dotConfigFileNames` in each one's `.config` directory (other than the home directory's) when it isn't a symlink, or get `dprint/fileInfo` again when a document whose configuration file is outside the workspace folders (including the global one), or that has none, is focused or saved.

A `dprint/format` result's `claim` (including a `failed` one's) and a `dprint/approve` result's `status` replace what was known before.

A configuration file is identified by its `uri` and the directory it's used for (`dirUri`), which the server always provides together, so key what's known about one by both. The global configuration file has results for each directory it's used for (ex. each drive). Send the `dirUri` with the `uri` when it's known (ex. from a result); it's needed for the global configuration file and the `cli` one, but not for one found in the workspace.

## Approving

Opening a project isn't choosing to run what its configuration file says, so the server only uses a configuration file once the user approves what it needs. Until then nothing is formatted with it, not even by its Wasm plugins, because formatting only some files would differ from the CLI.

**Downloads** - A download could tell a server the configuration file chose that the project was opened, so without approval the server only downloads plugins and extended configuration files (including each redirect) from these domains and their subdomains: `registry.npmjs.org`, `plugins.dprint.dev`, `github.com`, `githubusercontent.com`, and the settings file's `editor.trustedDomains` (none at all when the settings file can't be used, see "Process plugins"). These are approved like downloads too:

- Reading a file on another machine (ex. a UNC path, a path in `/net`, or an automounter's directory), including through a symlink. That's each file a configuration file names (ex. in `plugins` or `extends`), each npm package in `node_modules` it uses, and what the server reads on its own: the configuration files it looks for, `.npmrc` files other than `~/.npmrc`, and `.gitignore` and `.git` files (which are ignored instead when remote). The configuration file's own share and a share already allowed don't need approval. Each read is its own `downloads` approval with `networkPath` items. A configuration file on another machine, whether the server found it or the editor sent it (ex. to `dprint/configInfo`), is its approval's `configFile`, with `onAnotherMachine` until it's allowed, and an ancestor directory's configuration file isn't used instead. A path that reaches another machine through a symlink is shown as `local -> remote` (ex. `C:\project\tools\plugin.wasm -> \\server\share\plugin.wasm`, where an `->` within a path is shown as `-\u{3e}`), and is asked about again when the symlink points elsewhere.
- A proxy that an `.npmrc` file other than `~/.npmrc` sets (`https-proxy` or `proxy`), because it would receive every download and the setting's credentials, which may include the user's environment variables (ex. `https://${GITHUB_TOKEN}@proxy.example`). Until it's allowed, downloads that would use it need approval rather than going another way. Each proxy is its own `downloads` approval with an `npmrcProxy` item, and allowing it allows that setting's text in that file.

What's in dprint's cache (ex. from `dprint fmt`) and plugins that are files aren't downloaded. Allowing a download allows its exact urls (and for an npm registry, every url under it, which its item's `registry` says) for the session, for every configuration file. Allowing a directory allows what's in it apart from anything below it that's denied. A denial of downloads that were each allowed exactly since isn't kept, and the `notice` says so. A decision that isn't `always` is only for the session, so another server (ex. another window or `dprint` executable) asks again.

**Process plugins** - They aren't sandboxed, so running them needs approval for the configuration file with each plugin's configuration, unless the settings file has `editor.allowProcessPlugins` set to `true` (see [the language server docs](../website/src/lsp.md)). The global configuration file's process plugins run without approval because it's the user's own. Running dprint with a configuration file (ex. `dprint fmt`, `dprint check`, `dprint config install`, `dprint config update`, or `dprint output-resolved-config`) or choosing its plugins with `dprint config add` or `dprint init` approves its process plugins. The approval doesn't cover the project files a plugin runs (ex. a script in dprint-plugin-exec's `commands`), which may change without asking.

When the settings file can't be read or has an unknown property or value in `editor` (ex. a newer dprint's setting), the server fails closed: it trusts no domains (not even the defaults) and ignores the stored `alwaysAllow` decisions for process plugins, but still applies the stored denials. An `alwaysAllow` decided then is still stored and used for the session. The approval's `message` says why.

The results of `dprint/format`, `dprint/fileInfo`, and `dprint/configInfo` then have the `approvalRequired` reason (see "Which documents dprint formats" for which documents) and `approvals`. Any result's `approvals` are what to ask about, but only when the user formats (see "Asking").

```ts
interface PendingApproval {
  /**
   * Identifies exactly what's shown, so it changes when that does (ex. a process plugin's configuration, or once its process
   * plugins are set up). It's the same in other servers of the same dprint version but may change between versions. Only send a
   * decision for the id that was shown.
   */
  id: string;
  /** Treat an unknown kind like "processPlugins" with its `message`. */
  kind: "downloads" | "processPlugins" | (string & {});
  /** A short description (ex. for a status bar item or a prompt's heading). */
  title: string;
  /** The configuration file the user approves for. */
  configFile: ConfigFileInfo;
  /** What's downloaded or run. */
  items: ApprovalItem[];
  /** The decisions the user can make, in the order to show them. */
  decisions: ApprovalDecisionOption[];
  /** Whether the user denied it, in which case only ask when they ask (ex. a command). */
  denied: boolean;
  /** What the user decided before, which is "deny" or "alwaysDeny" when it's denied. */
  previousDecision?: ApprovalDecision;
  /**
   * What to show when asking: one sentence naming the configuration file (and whether it's outside the workspace) and everything
   * approved with where each item is from, followed by any notes, because an editor may not show line breaks. It leaves out what
   * process plugins run with and doesn't ask a question, because the decisions do. Project text is in backticks and escaped (see
   * "Escaping"), including backticks and look-alikes (ex. `\u{60}`). Ex. "Formatting `/project/dprint.json` needs to run process
   * plugins that aren't sandboxed (`npm:@dprint/exec@0.7.4`)."
   */
  message: string;
  /** The configuration file's trusted configuration, including what it extends, which process plugins that weren't set up before run with. */
  trustedConfiguration?: object;
}

interface ApprovalItem {
  /**
   * The url, path, or npm specifier (ex. of a plugin, without its checksum), escaped (see "Escaping"). A path that reaches another
   * machine through a symlink is followed by ` -> ` and the remote path. For an `npmrcProxy`, it's the setting's text before its
   * environment variables are replaced.
   */
  specifier: string;
  /**
   * `config` is an extended configuration file, `npmrcProxy` a proxy an `.npmrc` file sets, and `networkPath` a file or directory
   * on another machine. An npm registry's item is `plugin` when what's downloaded from it isn't known. Show an unknown kind by its
   * specifier.
   */
  kind: "plugin" | "config" | "npmrcProxy" | "networkPath" | (string & {});
  /**
   * For the download of an npm registry itself, the registry (also the specifier): show that allowing it allows everything under
   * it. Otherwise, an npm plugin's registry when it isn't `https://registry.npmjs.org/` (ex. set by an `.npmrc`): show it with the
   * specifier, which doesn't say where the plugin is from. Escaped like the specifier.
   */
  registry?: string;
  /** The `.npmrc` file of an `npmrcProxy`, to show with it (ex. with a way to open it). */
  npmrcUri?: string;
  /**
   * The local path of a `networkPath` that reaches the other machine through a symlink (before ` -> `), which may be shown with it.
   * Don't open it before it's allowed, because that reads the other machine.
   */
  localUri?: string;
  /** The plugin when it was set up before. */
  plugin?: FilePlugin;
  /** The configuration a process plugin that was set up before runs with (ex. dprint-plugin-exec's commands). */
  configuration?: unknown;
}

interface ApprovalDecisionOption {
  /** What `dprint/approve` is sent with. */
  decision: ApprovalDecision;
  label: string;
  /** Whether it allows what's approved (otherwise it denies it). */
  allows: boolean;
  /** Whether it's stored in the approvals file after the session ends (otherwise it's only for the session). */
  always: boolean;
  /** What it does, shown with it (ex. as a tooltip, or in the message when there are no tooltips). */
  description: string;
}

/** Show an unknown decision by its option's `label` and `description`. */
type ApprovalDecision = "allow" | "alwaysAllow" | "deny" | "alwaysDeny" | (string & {});
```

### Escaping

The server escapes project text in `specifier`, `registry`, and `message` so it can't change how what's around it reads:

- These are shown as `\u{<lowercase hex>}` (ex. `\u{a}` for a newline): control characters (`Cc`), separators other than a plain space (`Zs`, `Zl`, `Zp`), format characters (`Cf`, ex. the bidi override U+202E or a zero width space), other `Default_Ignorable_Code_Point`s, code points unassigned in the server's Unicode version (`Cn`, currently Unicode 16), and a combining mark (`Mn` or `Me`) at the start. In JavaScript: `/[\p{Cc}\p{Cf}\p{Cn}\p{Z}\p{Default_Ignorable_Code_Point}]/u` for any character but a space, and `/^[\p{Mn}\p{Me}]/u` for the first one. A client with other Unicode tables escapes slightly differently.
- A backslash followed by `u{` is shown as `\u{5c}`, and any other backslash as it is, so Windows paths stay readable (ex. `C:\new`).
- A `networkPath` specifier is also escaped like `message` text (backticks and look-alikes, and a combining mark at the start of each path), so a `localUri` shown next to it with your own escaping may differ (ex. `C:\proj\a\u{60}b\x -> \\srv\s\x` next to ``C:\proj\a`b\x``).
- A path that isn't valid Unicode can't be allowed, because its text can't be told apart from other paths: its approval only offers denying it, and its `message` says why.

### Asking

Ask when the user wants to format with the configuration file (ex. a format on save or a command), never when a project is opened. A format the user didn't run as a command only asks when the result's `claim` is `true`, or when the user chose the extension as the language's formatter (ex. VS Code's `editor.defaultFormatter`).

- Show the approval's `message` (instead of the `approvalRequired` result's), ask the same way about an unknown kind, and only offer its `decisions`.
- Because the `message` may be cut off, also show the configuration file (and that it's outside the workspace when `inWorkspace` is `false`) and what will be downloaded or run: each item's `specifier` (with an `npmrcProxy`'s `npmrcUri` and a `networkPath`'s `localUri`), `registry`, and `configuration`, and the `trustedConfiguration`, which includes what the configuration file extends. It may be collapsed (ex. behind a "Show Configuration" button) as long as the user can see it before deciding.
- Before it's allowed, don't open or read what's on another machine: a `networkPath` item's `localUri`, and a `configFile` whose `onAnotherMachine` is `true`.
- Also show that the configuration file needs approval somewhere that doesn't interrupt (ex. a status bar item that asks when clicked), because a denied approval isn't asked again and its documents aren't formatted. Get `dprint/pendingApprovals` for it whenever the results are gotten again, once they came back.
- Close a prompt whose approval isn't pending anymore in its server (ex. decided in another window). Don't count it as asked, get its configuration file's results again (or `dprint/pendingApprovals` with its `configFile`), and format the documents that waited on it when its `status` is `ready`.

Don't let a "save all" turn into a storm of prompts:

- Show one prompt per approval `id` at a time per server, shared by every format of that server waiting on it. Keep a prompt, its asked-once marks, and its decision with the server that asked; don't share them with or send the decision to another server, even for the same `id`, because session decisions aren't shared and another dprint version may offer other `decisions`.
- A format the user didn't run as a command asks about the same approval at most once until the user decides, even if the prompt is closed without deciding, and across restarts within the session. The same approval is the same configuration file (`uri` and `dirUri`), `kind`, and set of item `specifier`s (in any order), not the same `id`. Once the user picks a decision, stop counting it as asked whatever `dprint/approve` returns, so it's asked again if it's pending again later (ex. a process plugin's configuration changed). A command or the status bar item always asks, including about what was denied.
- Don't make a save wait on the prompt. Once approved, format the document's current text again, only apply the edits when the document didn't change meanwhile, and save it again when it was saved before and is unchanged since.

When several things need approval at once, ask one at a time, or show them together and send the decisions one at a time, each only while its `id` is still in the previous result:

- Take what to ask next from the `dprint/approve` result, which is what needs approval now (ex. a configuration file a downloaded one extends).
- Stop when nothing is left to ask: nothing that isn't `denied` for a format the user didn't run as a command, and nothing at all for a command or the status bar item. After a decision that allows, format the document again when the `status` is `ready`; when it isn't (ex. a download revealed a configuration error), show its `message` like a command's result.
- After a successful denial, don't ask about anything else in that request, because the configuration file isn't used. When the approvals were shown together, still send the user's other decisions while their ids are in the result.

### When `dprint/approve` fails

`-32803` means what was approved changed before the decision arrived, so it wasn't used. After a cancellation or another error, it may or may not have been used. After any failure, sending the decision at most twice for the same `id` to the same server:

1. Get what's pending: `data.current` for a `-32803` that has it, or else `dprint/pendingApprovals` with the approval's `configFile`. Handle it like a `dprint/approve` result.
2. When the approval with the same `id` is still pending and not `denied`, send the same decision again once. Don't send an allowing decision again for one that's `denied`, which would replace a denial made since (ex. in another window).
3. Otherwise, for a decision that doesn't allow, send the same denial once for an approval that replaced it, if there's one of the same `kind` and `configFile` that isn't `denied`, offers that decision, and whose item `specifier`s are all among the denied approval's. Don't ask about anything else in that request. Never send an allowing decision the user didn't make for what's shown.
4. Otherwise, for a decision that allows, ask about what's next like after a `dprint/approve` result.
5. Only show the original error when the decision is still pending afterwards (the same `id` is pending and not `denied`) or getting what's pending failed. When an allowing decision's approval is `denied` afterwards, tell the user it was denied since instead.

### Stored decisions

`always` decisions are stored in `editor-approvals.json` next to the settings file (`files.approvalsUri`), which every installed dprint shares and the server watches. Its format and locking rules are documented on `EditorApprovals` in the source. Don't edit it from an extension; use `dprint/forgetDecisions`.

- When a decision can't be stored (ex. the lock can't be taken, or the file is from a newer dprint this one can't write to), it's only used for the session and the result's `notice` says so. A version that can't use the file still applies its denials.
- The most recent decision is used, but an `alwaysDeny` stored after a server received a session allow replaces it, whatever their times say.

## Methods

A request that can't be handled gets an error with one of these codes:

- `-32602` (InvalidParams) - The params can't be handled (ex. an `overrideConfig` property that can't be overridden, a decision the approval doesn't have, or a `configFile` that isn't a configuration file).
- `-32803` (RequestFailed) - What `dprint/approve` was sent for isn't pending anymore (see "When `dprint/approve` fails"). Its `data` is `{ kind: "approvalChanged", current?: ConfigApprovals }`, where `current` is for the approval's configuration file and is missing when that isn't known (ex. an id the server didn't provide). Recognize it by `data.kind`, and treat it like `-32603` without it.
- `-32800` (RequestCancelled) - Cancelled with `$/cancelRequest`. Don't show it.
- `-32601` (MethodNotFound) - Unknown method, or the server handles the client like a language client.
- `-32603` (InternalError) - Anything else (ex. a configuration file couldn't be read), which doesn't mean the server stopped. Treat an unknown code like this.

An error's `data` is an object with a `kind`, where an unknown kind is ignored. Params and results are objects that may have fields added later, so a method without params takes `{}`.

### `dprint/format` (request)

Formats a document.

```ts
interface FormatParams extends DocumentParams {
  text: string;
  /** The ranges to format, which is the whole text when not provided and nothing when empty. */
  ranges?: Range[];
  /**
   * The editor's indentation, used instead of the configuration file's. Only send it when the user wants the editor's to win (ex. an
   * extension setting), because the result otherwise differs from `dprint fmt`. Nothing else can be overridden, because it could
   * have a plugin do something the user didn't approve. `indentWidth` is a positive integer.
   */
  overrideConfig?: { indentWidth?: number; useTabs?: boolean };
}

interface FormatResponse {
  /** `null` when there's no configuration file. */
  configFile: ConfigFileInfo | null;
  result: FormatResult;
}

type FormatResult =
  /** Non-overlapping edits of the sent text in order, which are empty when it's already formatted. */
  | { kind: "formatted"; edits: TextEdit[]; notice?: Notice }
  | ({ kind: "notFormatted" } & NotFormatted)
  /** A plugin couldn't format it (ex. a syntax error) or failed (ex. it panicked). */
  | {
    kind: "failed";
    reason: "formatError" | "pluginError" | (string & {});
    message: string;
    show: ShowPolicy;
    severity: Severity;
    /** Whether to keep acting as the document's formatter, which is currently `true`. */
    claim: boolean;
    actions?: Action[];
    /** The plugin when it's known. */
    plugin?: FilePlugin;
  }
  /** An unknown kind (see below). */
  | {
    kind: string & {};
  };

/** Something to tell the user about a request's result (ex. a warning about a formatted document, or a decision that couldn't be stored). */
interface Notice {
  message: string;
  show: ShowPolicy;
  severity: Severity;
  actions?: Action[];
}

/** Something the user can do about a message, offered with it. Leave out an unknown kind. */
type Action =
  /** Opens the file in the editor (ex. the configuration file that has an error). */
  | { kind: "openUri"; label: string; uri: string }
  | { kind: string & {}; label: string };

/** Why a document isn't formatted, which is the same for each method. */
interface NotFormatted {
  reason: NotFormattedReason;
  message: string;
  show: ShowPolicy;
  severity: Severity;
  /** Whether to keep acting as the document's formatter so the user is told why, instead of leaving it to the editor's other formatters. */
  claim: boolean;
  /** What needs to be approved to format it (see "Approving"). */
  approvals?: PendingApproval[];
  actions?: Action[];
}

/** When to show a message, which is only for a request the user made, not one the editor made on its own. Treat an unknown value like "command". */
type ShowPolicy =
  /** Each time (ex. a plugin failed). */
  | "always"
  /** Only for a command the user ran (ex. a syntax error, which a format on save would otherwise show each time). */
  | "command"
  /** Once per configuration file until the message changes or a non-`schemaOnly` `dprint/configChanged` for it (ex. a configuration error). */
  | "oncePerConfigFile"
  | (string & {});

/** How to show a message. Treat an unknown value like "warning". */
type Severity = "error" | "warning" | "info" | (string & {});

type NotFormattedReason =
  /** No config file in an ancestor directory and no global config file. */
  | "noConfigFile"
  /** No config file in an ancestor directory and the global config file isn't used. */
  | "globalConfigNotAllowed"
  /** The config file doesn't format it (ex. its includes and excludes don't match it). */
  | "notMatched"
  /** No plugin in the config file formats it. */
  | "noPlugin"
  /** A plugin that formats it doesn't support formatting a range. */
  | "rangeFormattingNotSupported"
  /** A plugin's configuration has problems, which are in the config file's diagnostics. */
  | "configDiagnostics"
  /** The config file failed to resolve. */
  | "configError"
  /** Something needs to be approved first, which the approvals have. */
  | "approvalRequired"
  /** It can't be formatted (ex. its uri isn't a file, an untitled document, or a notebook cell). */
  | "unsupportedDocument"
  /** It isn't a file, so its language is needed (see "Documents"). */
  | "languageRequired"
  /** The config file that was asked about doesn't exist (ex. it was deleted). */
  | "configFileMissing"
  | (string & {});
```

Ranges are formatted in order, and overlapping ones are merged. A plugin that supports range formatting decides what to format around a range, and otherwise the range is expanded to whole lines. The document's line endings are kept (a text without a line break gets the configuration's).

Show the `message` of a result or `notice` as its `show` and `severity` say, with its `actions`; the server doesn't show anything itself. A format the editor can't tell apart from one the user didn't run as a command (ex. a formatter that's also run on save) isn't a command. For `oncePerConfigFile`, the configuration file is the result's `configFile` (`uri` and `dirUri`), or the message alone when there isn't one (ex. a `dprint/forgetDecisions` `notice`).

Treat an unknown result kind like `notFormatted` with the fields it has, the same as for a `FormatStatus`: ask about its `approvals`, don't claim the document without `claim`, and only show its `message` for a command when it has no `show` (or a generic message when it has no `message`).

### `dprint/fileInfo` (request)

Gets how a document is formatted.

```ts
interface FileInfoParams extends DocumentParams {
  /** The document's text, which is otherwise read from the file when needed (ex. an extensionless file's shebang). */
  text?: string;
}

interface FileInfoResult {
  configFile: ConfigFileInfo | null;
  /** The plugins that format it in order, which is empty when it isn't formatted. */
  plugins: FilePlugin[];
  status: FormatStatus;
}

/** Treat an unknown kind like "notFormatted" with the fields it has, which doesn't claim the document without `claim`. */
type FormatStatus = { kind: "ready" } | ({ kind: "notFormatted" } & NotFormatted) | { kind: string & {} };

interface FilePlugin {
  name: string;
  version: string;
  configKey: string;
}

interface ConfigFileInfo {
  /** A file uri, because the server only finds local configuration files and is only started with one. */
  uri: string;
  /**
   * The directory whose files it formats: its own directory (even when it's a symlink to a file elsewhere) unless that's a
   * `.config` directory other than the home directory's, and the directory it's used for when it's the global configuration file
   * (ex. a drive's root).
   */
  dirUri: string;
  /** Found in an ancestor directory, the global configuration file, or the one from `dprint lsp --config`. Treat an unknown kind like "local". */
  kind: "local" | "global" | "cli" | (string & {});
  /** Whether it's in a workspace folder, which the global configuration file and one in an ancestor directory aren't. */
  inWorkspace: boolean;
  /**
   * `true` when it was found on another machine and reading it isn't allowed yet (see "Approving"), and left out otherwise. Don't
   * open or read it then (ex. a prompt button or an "open configuration file" command), including from an older result or action,
   * while the latest `dprint/configInfo` or `dprint/fileInfo` result with its `uri` and `dirUri` has it. A non-`schemaOnly`
   * `dprint/configChanged` that names it (or no configuration file) and a server restart make it unknown again. The server sends
   * `dprint/configChanged` once it's allowed and when a configuration file becomes remote (ex. a `git pull` made it a symlink to a
   * share), and takes back its diagnostics itself. This is a best effort (see "Security").
   */
  onAnotherMachine?: boolean;
}
```

### `dprint/configInfo` (request)

Gets the plugins of a configuration file, given either the configuration file or a document it's used for (not both, or neither, which fails with `-32602`):

```json
{ "configFile": { "uri": "file:///home/user/project/dprint.json" } }
```

```json
{ "textDocument": { "uri": "file:///home/user/project/src/file.ts" } }
```

```ts
type ConfigParams = { configFile: ConfigFileIdentifier; workDoneToken?: string | number } | DocumentParams;

interface ConfigFileIdentifier {
  uri: string;
  /**
   * The directory it's used for (its `dirUri`). When not provided, it's the root of the server's current directory for the global
   * configuration file, and the server's current directory for the `cli` one. For `dprint/decisions` and `dprint/forgetDecisions`,
   * it means every directory the configuration file is used for.
   */
  dirUri?: string;
}

interface ConfigInfoResult {
  configFile: ConfigFileInfo | null;
  /** `null` when they aren't known (see "Which documents dprint formats"). */
  plugins: PluginInfo[] | null;
  filePatterns: FilePattern[];
  /** See "Which documents dprint formats". */
  otherFiles: { kind: "shebangs" | "associations" | (string & {}); values: string[] }[];
  /**
   * The local files to watch by their directories (see "Lifecycle"): the files it was resolved from (ex. one it extends), and its
   * own canonical path when the `configFileGlobs` watcher doesn't report changes there (it's a symlink or in a symlinked `.config`
   * directory, its names are in another case on disk, or it's reached through a directory symlink the editor doesn't watch
   * through). See "Paths". A file whose directory may not be read (ex. it's on another machine now) is left out.
   */
  watchUris: string[];
  status: FormatStatus;
}

interface FilePattern {
  /** Relative to `configFile.dirUri` (see "Which documents dprint formats"). */
  glob: string;
  /** The config keys of the plugins that format the documents. */
  configKeys: string[];
}

interface PluginInfo extends FilePlugin {
  /** Without the leading period. */
  fileExtensions: string[];
  /** The file names it formats regardless of extension. */
  fileNames: string[];
  /** Whether it formats a file in addition to the plugin that formats it (ex. sorting a package.json's keys). */
  additive: boolean;
  /** Whether it decides what to format around a range, formats the range's whole lines, or doesn't format a range. Treat an unknown value like "unsupported". */
  rangeFormatting: "supported" | "wholeLines" | "unsupported" | (string & {});
  helpUrl: string;
  /** Where its newer version is said to be. */
  updateUrl?: string;
}
```

A `configFile` needs to have a configuration file's name, be the global configuration file, or be the `cli` one (which is then the only one). Its status is `configFileMissing` when it doesn't exist. It's used even when another configuration file in its directory is the one dprint uses (ex. a `dprint.jsonc` beside a `dprint.json`).

The status is for the configuration file: `ready`, or why none of its documents are formatted (ex. `configError`, `approvalRequired`, or `configFileMissing`). `dprint/fileInfo` has a document's reasons (ex. `notMatched`).

### `dprint/configSchema` (request)

Gets the JSON schema (draft 7) of a configuration file (`ConfigParams`) with its plugins' schemas, for completions and validation. A plugin's schema is only downloaded from a trusted domain.

```ts
interface ConfigSchemaResult {
  schema: object;
  /**
   * Whether it has each plugin's schema. It doesn't when the configuration file can't be used (ex. it needs approval) or a schema
   * didn't download in time, and then a `schemaOnly` `dprint/configChanged` is sent once it did.
   */
  complete: boolean;
}
```

### `dprint/approve` (request)

Sends the user's decision about a pending approval.

```ts
interface ApproveParams {
  /** The approval's `id`. */
  id: string;
  /** The `decision` of one of the approval's `decisions`. */
  decision: ApprovalDecision;
  workDoneToken?: string | number;
}

interface ApproveResult extends ConfigApprovals {
  /**
   * What to tell the user about this or an earlier decision for the configuration file. Ex. a denial wasn't kept because what it
   * denied was each allowed exactly since, an `always` decision couldn't be stored (see "Stored decisions"), the session's file
   * couldn't be written or deleted (so earlier session decisions may come back after a restart), or the session's decisions aren't
   * kept yet (see "Session id").
   */
  notice?: Notice;
}

/**
 * What needs to be approved for a configuration file now and whether it's used. It's the result of `dprint/approve` and of
 * `dprint/pendingApprovals` with a `configFile`, and the `data.current` of `-32803`. Handle each the same way.
 */
interface ConfigApprovals {
  /** `null` when it can't be resolved anymore (ex. it was deleted). */
  configFile: ConfigFileInfo | null;
  /**
   * What needs approval now, which is empty when nothing does. After a denial that was kept, it has what was denied; otherwise it
   * may have something else that needs approval.
   */
  approvals: PendingApproval[];
  /** Whether the configuration file is used now, which isn't "ready" when it has an error or was deleted. Get a document's `dprint/fileInfo` to know whether it's formatted. */
  status: FormatStatus;
}
```

The decision is used once the server receives it, even when the request is cancelled. It's for the approval's `configFile`, even when the document it was asked about for uses another one now. Allowing downloads and sets up what was allowed before the configuration file is resolved again for the result. It fails with `-32803` when nothing is pending with the id.

### `dprint/pendingApprovals` (request)

Gets what needs to be approved, including what was denied (ex. for a status bar item or a command).

```ts
interface PendingApprovalsParams {
  /** Every configuration file that was used when not provided. */
  configFile?: ConfigFileIdentifier;
  workDoneToken?: string | number;
}

/** A `ConfigApprovals` exactly when the params have a `configFile`. */
type PendingApprovalsResult = { approvals: PendingApproval[] } | ConfigApprovals;
```

### `dprint/forgetDecisions` (request)

Forgets what the user decided for a configuration file, or every configuration file, including the `always` decisions and those from running dprint (ex. `dprint fmt`), so the user is asked again. Allowed downloads stay allowed for the session. It responds once the configuration files were resolved again, and sends `dprint/configChanged` for those whose results changed.

It forgets this server's session decisions and the stored ones (waiting for an `always` decision it's still storing), but not another server's session decisions. So an extension with several servers sends it to each server that may have used the configuration file (or all of them, without a `configFile`), and forgets what it asked about for each (see "Lifecycle"). An `alwaysAllow` of process plugins that another server received before they were set up is only stored once that server sets them up, so it may still be stored afterwards.

```ts
interface ForgetDecisionsParams {
  /** Every configuration file when not provided. Without a `dirUri`, it's for each directory it's used for (ex. the global configuration file on each drive). */
  configFile?: ConfigFileIdentifier;
  workDoneToken?: string | number;
}

interface ForgetDecisionsResult {
  /** The decisions that were forgotten. */
  decisions: Decision[];
  /**
   * What to tell the user about forgetting them. Ex. the stored decisions couldn't be forgotten (including when the approvals file
   * is from a newer dprint, so only the session's were), the session's file couldn't be written or deleted (so they may come back
   * after a restart), or the session's decisions aren't kept yet (see "Session id").
   */
  notice?: Notice;
}
```

### `dprint/decisions` (request)

Gets what the user decided for a configuration file, or every configuration file (ex. for a command that shows them). A process plugin has the decision that's used for it, which is the most recent. An allow may be for an earlier configuration of the plugin (which is then asked about again when used), while a denial is for any configuration.

A server only lists its own session's decisions and the stored ones, so an extension with several servers gets them from each and keeps one of each `always` decision (another server may provide it by another path of the configuration file). Forgetting is per configuration file, not per decision: `dprint/forgetDecisions` with a decision's `configFile` forgets all of that configuration file's decisions, so offer it per configuration file (ex. "Forget the decisions of `<path>`") and send it to each server that may have used it.

```ts
interface DecisionsParams {
  /** Every configuration file when not provided. Without a `dirUri`, it's for each directory it's used for. */
  configFile?: ConfigFileIdentifier;
}

interface DecisionsResult {
  decisions: Decision[];
  /** Whether the settings file allows every process plugin without asking (`editor.allowProcessPlugins`). */
  allowProcessPlugins: boolean;
}

interface Decision {
  /** Show an unknown kind with its `specifiers`. */
  kind: "downloads" | "processPlugins" | (string & {});
  /**
   * The configuration file it's for, to send to `dprint/forgetDecisions`. It's missing for a download allowed for every
   * configuration file, and for a process plugin decision whose configuration file the server can't make a uri of (ex. stored on
   * another operating system). Such a decision can't be forgotten on its own, so don't offer to, and never send
   * `dprint/forgetDecisions` without a `configFile` for it, which forgets everything. `dirUri` is missing when it's for every
   * directory the configuration file is used for. `uri` is an https or npm uri for a decision made by running dprint with a remote
   * or npm configuration file (ex. `dprint fmt --config <url>`).
   */
  configFile?: ConfigFileIdentifier;
  /** The urls, paths, or specifiers it's for, escaped like an approval item's `specifier`. */
  specifiers: string[];
  /**
   * One item per specifier in the same order, like an approval's, so a proxy or a remote path can be shown with its file
   * (`npmrcUri` or `localUri`). Usually provided for downloads, and missing when not known (ex. a denial another version kept).
   */
  items?: ApprovalItem[];
  allows: boolean;
  /** Whether it's remembered after the session ends, which includes running dprint with the configuration file. */
  always: boolean;
}
```

### `dprint/reset` (request)

Forgets the configuration files and stops the plugins, which are set up again when next used (ex. after a plugin stopped responding or was changed), and has stale remote files they're resolved from downloaded again (all of them with `--reload`). Unlike the restart command, the session's decisions are kept. It takes `{}` and returns `{}`. Get the configuration files' plugins again afterwards, because a changed plugin may format other files.

### `dprint/configChanged` (notification from the server)

```ts
interface ConfigChangedParams {
  /** Every configuration file when missing. */
  configFile?: ConfigFileIdentifier;
  /** Missing without a `configFile`. Treat an unknown change like "changed". */
  change?: "created" | "changed" | "deleted" | (string & {});
  /** Whether only its schema changed (ex. a plugin's schema downloaded), so get the schema again but not the plugins. */
  schemaOnly?: boolean;
}
```

See "Which documents dprint formats" for what to get again.

### Language server protocol methods

The server also uses these, which an extension should handle:

- `window/logMessage` - What the server logs. Stderr only has what can't be logged (ex. a crash).
- `window/showMessage` - Something to show that isn't in a result. Not sent to an extension now (problems are in results, ex. a `notice`), but a later version may.
- `window/workDoneProgress/create` and `$/progress` - Long-running work (ex. downloading a plugin), when the client supports it. Work during a request with a `workDoneToken` is reported on that token as the total of everything in progress (ex. a download that started earlier), and work that starts then is reported on the server's own token once the request is done.
- `textDocument/publishDiagnostics` - A configuration file's problems (ex. an unknown property), replacing the ones published before. A problem without a known position is at the start of the file. The server takes them back by publishing empty ones, including to a uri that leads to another machine now. A library's handler may look the uri up on disk, which follows symlinks (ex. Neovim's), which is the library's own handling (see "Security"). An extension that replaces the handler finds the document by comparing names as text (ex. Neovim's listed buffer names), ignores empty problems for a uri without a document, and only creates an entry for problems that aren't empty, which the server only publishes for a configuration file that may be read.
- `$/cancelRequest` - Cancels a request (ex. a format that isn't needed anymore).
- `workspace/didChangeWatchedFiles` - See "Lifecycle".
- `client/registerCapability` and `client/unregisterCapability` - With `watchFiles`, the files the server needs to be told about.

<!--
## Design decisions

These are kept here for the maintainers and aren't part of the protocol.

- `dprint/approve` takes one decision instead of a batch. Approvals depend on each other: allowing a download is what reveals what needs to be approved next (ex. a configuration file the downloaded one extends or the process plugins of a downloaded configuration file), so applying several decisions atomically doesn't fit. The client sends one at a time and takes the next ids from each result.
- The server watches dprint's own files (its settings file, `editor-approvals.json`, and the global configuration file) instead of asking the editor to, because their location depends on the system and every editor extension would otherwise need to find and watch them. It watches their directories with the system's file watcher, and checks them every 30 seconds instead when it can't (ex. the directory doesn't exist yet). The workspace is still watched by the editor, which already watches it.
- A config file is resolved again when the editor says it changed only when it was resolved before, because resolving it may download and set up plugins without the user doing anything.
- Whether to keep claiming a document (`claim`) and when and how to show a message (`show` and `severity`) are decided by the server instead of the client from the reason, so that a reason added later is handled right by older clients, which would otherwise fall back to treating it like `notMatched` or `pluginError`.
- The server uses canonical paths but provides the editor's (learned from the workspace folders and the uris it's sent), so an editor extension never needs to canonicalize or map paths itself (ex. diagnostics published to a path through a symlink show on the document the user has open).
- A client that sent the `dprint` capability without a version the server supports isn't handled like a language client, because its language client library would start using the text document methods before the extension could stop it.
- An approval's id is a SHA-256 hash of everything that's approved, including the trusted configuration of the config file for a process plugin that isn't set up yet and the registry of an npm plugin that isn't from the default one, so a decision is never used for something other than what was shown.
- When the server doesn't support any of the client's versions, it still provides the capability with its `versions` so that a client can tell it apart from a dprint that's older than the `dprint/*` methods, which can't be added after a version is dropped.
- `-32803` (RequestFailed) is used for an approval that changed instead of a code in JSON-RPC's reserved range, which a client library may use for its own errors.
- A configuration file that's asked about is resolved even when another one in its directory is what dprint uses for the directory (ex. a `dprint.jsonc` beside a `dprint.json`). Detecting that isn't worth maintaining for a mistake that's rare and obvious to the user.
- A proxy that an `.npmrc` file other than `~/.npmrc` sets is approved like a download instead of being ignored, so a project that needs its proxy (ex. on a company's network) works once the user allows it, and a download never silently goes another way than npm would send it. Only `~/.npmrc` is trusted because the files in the ancestor directories may be the project's (ex. the root of a monorepo) or anyone's (ex. in `/tmp`), and dprint doesn't read npm's global configuration file. What's allowed is the file with the text of its setting before its environment variables are replaced, which is what the user saw, so a changed setting is asked about again, while a changed value of an environment variable isn't. Like a download, it's only allowed for the session. Each proxy is its own approval so its message starts with it and its credential warning isn't pushed past where an editor cuts off a message by long urls (VS Code cuts a notification at 1000 characters), and so the user can allow a plugin's url without the proxy. It's still the `downloads` kind because it's decided like one, and an older editor extension asks about a kind it doesn't know like "processPlugins".
- The server keeps what the user decided for the session in a file for the client's session id, instead of the client sending the session's decisions again to a server that's started again, because the bookkeeping for that is easy for a client to get wrong (which ids an approval had before, which decisions replace each other, and whether an `always` decision was stored, including one stored once its process plugins were set up by a later approval), and a mistake silently allows or denies what the user decided otherwise since. The server already knows each of these, so a file it writes after each change makes a server that's started again behave like the one that exited, and a client only keeps which approvals it asked about. The file is per session id, which is unique to a server process, so servers of different sessions don't share a lock. Two servers that are running with the same session id are a mistake of the client (or a server that's started again before the previous one exited), so the second one doesn't read or write the file until the first one exited, which its lock of the session tells, and then writes its own decisions to it right away, so the file never has the decisions of the other server. A server that's started again after a crash also leaves out the session's process plugin allows and set-up grants when the approvals file changed since the server that exited last read it (or that isn't known), because a stored decision that replaced them may have been removed since (ex. an `alwaysDeny` that was then forgotten in another window), which a server that kept running would have seen. It keeps the allowed downloads, because the approvals file has no download decisions. The id only selects the file in the user's cache directory, so it can't point the server at a file of the project.
- A decision isn't remembered by url or domain beyond the session, and allowing a download only allows its urls, because a broader scope (ex. trusting a domain) is the settings file's `editor.trustedDomains`, which the user edits on purpose.
-->
