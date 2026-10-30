# Developing an Editor Extension

An editor extension formats documents with the `dprint lsp` command, which is a language server. A language client formats documents with the text document methods of the language server protocol (see [the language server docs](../website/src/lsp.md)). An editor extension uses the `dprint/*` methods described here instead, which are only sent a document's text when formatting it, so the server doesn't keep the documents. They also say which documents dprint formats and let the editor extension ask the user what needs to be approved.

The `dprint/*` methods were introduced in dprint 0.65. Editor extensions for older versions used `dprint editor-service` (see [schema version 5](./editor-extension-development-schema-5.md)), which formats the files of a configuration file that the editor found.

An editor extension usually does the following, which the sections below describe:

1. Start `dprint lsp`, send `initialize` with the `dprint` capability, check the `version` of the server's capability, and send `initialized` (see "Starting the server").
2. Watch the configuration files (`configFileGlobs`) and the files they were resolved from (`watchUris`), and send `workspace/didChangeWatchedFiles` when they change (see "Lifecycle").
3. When a document is opened, get its `dprint/fileInfo`, which says whether dprint formats it (`status`) or the editor extension should act as its formatter anyway (`claim`), and which configuration file is used for it (`configFile`). Get `dprint/configInfo` for that configuration file, which says which other documents it may format (see "Which documents dprint formats").
4. Format a document with `dprint/format`. When something needs to be approved first (the `approvalRequired` reason), ask the user and send their decision with `dprint/approve` (see "Approving").
5. When the server sends `dprint/configChanged`, get the `dprint/configInfo` and `dprint/fileInfo` results again.
6. Show the `message` of a result like its `show` and `severity` say (see `dprint/format`).

## Starting the server

Start one `dprint lsp` process for each `dprint` executable the editor uses (usually one for the whole editor window, not one for each workspace folder) and send the `initialize` request with the `dprint` experimental capability:

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

Then send the `initialized` notification, which is when the server registers what it needs from the client (ex. the files to watch with the `watchFiles` feature).

`versions` are the versions of the `dprint/*` methods the editor extension supports. The server uses the highest one that it also supports, which is the `version` in the `dprint` experimental capability of the `capabilities` of its result:

```json
{
  "experimental": {
    "dprint": {
      "version": 1,
      "versions": [1],
      "files": {
        "settingsUri": "file:///home/user/.config/dprint/settings.json",
        "approvalsUri": "file:///home/user/.config/dprint/editor-approvals.json",
        "globalConfigUri": "file:///home/user/.config/dprint/dprint.json"
      },
      "configFileGlobs": ["**/dprint.json", "**/dprint.jsonc", "**/.dprint.json", "**/.dprint.jsonc"]
    }
  },
  "positionEncoding": "utf-16"
}
```

Use the server only when its capability has a `version`:

- When the capability doesn't have a `version`, the server doesn't support any of the editor extension's versions. It doesn't advertise anything else (ex. the text document methods), so a language client library doesn't start using it, and every method fails with `-32601`. Show the capability's `message` to the user, which says whether dprint or the editor extension needs to be updated, and stop the server with `shutdown` and `exit`. Don't fall back to an older protocol (ex. schema version 5), because that version of dprint may not have it anymore.
- When the result doesn't have the capability, it's a version of dprint older than 0.65, which handles the client like a language client, so stop it right away (a language client library may already use what it advertises) and fall back to [schema version 5](./editor-extension-development-schema-5.md) or tell the user to update dprint.

The result's `serverInfo.version` is the version of dprint (ex. to log it or to tell apart the servers of different executables).

```ts
interface ClientCapability {
  /** The versions of the `dprint/*` methods the editor extension supports, which are positive integers. */
  versions: number[];
  /** The optional behavior it supports (see "Features"). */
  features?: { [name: string]: true | object };
}

interface ServerCapability {
  /** The version of the `dprint/*` methods the server uses, which is missing when it doesn't support any of the client's. */
  version?: number;
  /** The versions the server supports. */
  versions: number[];
  /** The features it uses of the client's, which is missing when it doesn't use any. */
  features?: { [name: string]: true | object };
  /** Where dprint's own files are, which is missing without a `version`. */
  files?: { settingsUri?: string; approvalsUri?: string; globalConfigUri?: string };
  /** The configuration files to watch in the workspace folders, which is missing without a `version`. */
  configFileGlobs?: string[];
  /** What to tell the user when there isn't a `version`. */
  message?: string;
}
```

`files` are where dprint's own files are, so the editor extension can offer commands that open them (ex. to add a domain to `editor.trustedDomains` in the settings file, or to edit the global configuration file). A file is left out when its directory can't be found, and `globalConfigUri` is where the global configuration file would be created when it doesn't exist, so a command that opens it may offer to create it. Use `dprint/forgetDecisions` to forget what the user decided instead of changing the approvals file.

`configFileGlobs` are the configuration files to watch in the workspace folders (see "Lifecycle"), which may have names added later. They're empty when the server was started with a configuration file (`dprint lsp --config <path>`), which is the only one, so another configuration file in the workspace isn't one to the server (ex. `dprint/configInfo` fails with `-32602` for it). When an older server doesn't provide them, watch `**/dprint.json`, `**/dprint.jsonc`, `**/.dprint.json`, and `**/.dprint.jsonc`.

An editor whose extensions can't send custom requests or handle custom notifications (ex. Zed or Helix) uses `dprint lsp` as a language client instead (see [the language server docs](../website/src/lsp.md)).

One server formats the files of a whole workspace, and also the files outside the workspace folders (ex. a file opened on its own). For each file, the server finds the configuration file in an ancestor directory, or uses the global configuration file when that's allowed. The workspace folders (or the `rootUri` without them) are where untitled documents are formatted (see "Documents"), whether a configuration file is in the workspace (`inWorkspace`), and how the editor knows the paths of the files (see "Documents"). An editor extension built on a library that starts a server for each project (ex. eglot or lsp-mode) works too, but what the user decided for the session and the plugins that were set up aren't shared between the servers. Run it in the first workspace folder, or the home directory without one, because the current directory is where the global configuration file is used for when a `dirUri` isn't provided. The configuration is read again for each request, so an edited configuration file is used for the next request without restarting the server. Specifying `dprint lsp --config <path>` uses that configuration file for every file instead.

With the `dprint` capability, the server doesn't sync documents, register the text document methods, or provide completions for configuration files (the configuration schema method provides what's needed for that). Without a `version`, the `dprint/*` methods fail with `-32601`.

### Security

The editor extension decides what the server is started with, so a project can't choose it:

- Use the `dprint` executable the user configured, or otherwise the one on the path. Find the one on the path yourself and start it by its absolute path, only looking in the absolute directories of the path, because starting a bare `dprint` may run one in the current directory first (ex. Node's `child_process` and Neovim on Windows look for a `dprint.exe` in the current directory, which is the workspace folder).
- An editor extension may also use the one a project installed with npm (ex. `node_modules/@dprint/<platform package>/dprint` found from the workspace folder and its ancestors, which includes the root of an npm workspace when a package of it is opened) so that the project's version of dprint is used. That executable is from the repository, so running it is the same as running code from the repository. Only do this in an editor that has workspace trust (ex. VS Code's Restricted Mode) once the user trusts the workspace.
- Only take the executable, its arguments, and its environment variables from the user's or the machine's settings, or from the workspace's settings once the user trusts the workspace. A project could otherwise point `DPRINT_CONFIG_DIR`, `XDG_CONFIG_HOME`, `HOME`, or `APPDATA` at a directory in the repository with a settings file that allows process plugins (`editor.allowProcessPlugins`) or a global configuration file whose process plugins run without asking, or point `DPRINT_CACHE_DIR` at plugins it compiled. A proxy in the environment (ex. `HTTPS_PROXY` or `NPM_CONFIG_HTTPS_PROXY`) is used without asking, so the project could also have every download sent to a server of its choosing. Keep in mind that the editor's environment may come from the project (ex. direnv, or opening the editor from its directory).
- Show where a configuration file is when asking about it, and that it's outside the workspace when its `inWorkspace` is `false`, because a configuration file in an ancestor directory (ex. in `/tmp` or the root of a drive) may not be the project's.
- Show every string the server provides as plain text (ex. messages, labels, descriptions, titles, specifiers, uris, and configuration), because they may have text from a configuration file (ex. a plugin's URL or configuration). An editor that renders Markdown or HTML in a notification may otherwise show a link from the project, which may run an editor command (ex. VS Code's `[label](command:...)` links in notifications), so escape it (ex. break up the `](` of a Markdown link).

### Compatibility

The editor extension says the versions of the `dprint/*` methods it supports and the server responds with the one it uses, so a server can support an old version alongside a new one:

- `versions` is a non-empty list of positive integers. A version the server can't read (ex. `"2"`) is ignored. A capability that isn't an object with `versions`, or that doesn't have a version the server supports, has the server leave `version` out of its capability (see "Starting the server").
- A version is added for a change that an editor extension needs to know about (ex. one that would break it). Fields may be added to results, and values to the string unions of results (ex. kinds and reasons), within a version, so ignore fields that aren't known and handle values that aren't known like the methods below say. What's added later is in new fields when it's more information about something (ex. a warning about a document that's formatted), and a new kind of result is only for something an older client can handle with its fallback.
- The server provides what the editor extension needs to handle a value it doesn't know where it can (ex. whether to keep claiming a document, when and how to show a message, and whether a decision allows what's approved), so prefer those fields over deciding from the value.
- A field or a value of params that changes what the server does (ex. a new `globalConfig` value or an `overrideConfig` key) is only added with a new version or a feature, and the client only sends it when the server responded with that version or feature, because the server rejects a value it doesn't know (`-32602`) and ignores a field it doesn't know. The decisions are the exception because the server offers them (a client only sends one of an approval's `decisions`). Likewise, a new kind of result that an older client would handle wrongly with its fallback is only returned when the client sent the feature for it.
- A new notification or request from the server is only sent when the client sent the feature for it, which includes the language server protocol's (ex. `workspace/configuration`), because a client library may answer one by itself. Ignore a notification that isn't known, and respond to a request that isn't known with `-32601`.
- The server ignores a setting it doesn't know or with a value of the wrong type, so a setting may be sent to an older server. The `initializationOptions` are sent before the version is known, so a new value of a setting that needs a version or a feature is only sent with `workspace/didChangeConfiguration` once the server responded with it.
- The glob syntax (see "Which documents dprint formats"), the error codes, and the positions being UTF-16 don't change within a version.
- The strings the server provides are plain text, not Markdown (see "Security").
- Params that are optional may be `null`, which is the same as not providing them. Results provide every field that isn't optional, which may be `null` where the types say so, and leave out an optional field instead of making it `null`.

### Features

Optional behavior is opted into with `features`, an object of the features the client supports (`{ [name: string]: true | object }`, where an object has options). The server responds with the ones it uses in its capability's `features` (`true`, or the options it uses), which is left out when it doesn't use any, so a feature is only used when its name is in the server's `features`. A feature that isn't known is ignored, and a feature applies to whichever version the server uses.

```json
{ "experimental": { "dprint": { "versions": [1], "features": { "fileUri": true, "watchFiles": true } } } }
```

- `fileUri` - A document is formatted as the file of its `fileUri` instead of its uri (see "Documents").
- `watchFiles` - The server registers the files it needs to be told about with `workspace/didChangeWatchedFiles` instead of the editor extension watching them (see "Lifecycle"). This is for an editor extension built on a language client library that already handles that registration (ex. one for Neovim, Emacs, or Sublime Text). It's only used when the client supports registering it dynamically with relative patterns (`dynamicRegistration` and `relativePatternSupport`), so watch the files yourself when it isn't in the server's `features`. The configuration files in the workspace folders are registered with a glob, and the configuration files that were used and the files they were resolved from (their `watchUris`) are registered by their directory, which is registered again when they change.

### Lifecycle

- The server exits when the process of the `processId` of the `initialize` request exits, when the `exit` notification is received, or when its stdin is closed.
- Send `shutdown` and then `exit` to stop it, which stops the plugins. Clear the diagnostics it published once it exits, because they aren't published again until a configuration file is used.
- Keep the server running for as long as the editor uses it. It reads a configuration file again for each request that uses it, so there's no need to start it again when one is created, changed, or deleted, or when the workspace folders change (send `workspace/didChangeWorkspaceFolders`).
- The server doesn't watch the workspace. Send `workspace/didChangeWatchedFiles` when a configuration file (`configFileGlobs`) is created, changed, or deleted, when the `configFile.uri` of a `dprint/configInfo` result changes that's outside the workspace folders (ex. a configuration file in an ancestor directory of a workspace folder) or is the one the server was started with (`kind: "cli"`, which sends `dprint/configChanged` without a `configFile` because it's used for every file), and when one of its `watchUris` changes (ex. a configuration file it extends). The server resolves a configuration file that it resolved before again to publish its diagnostics, and sends `dprint/configChanged` when what a configuration file formats may have changed. It doesn't resolve one that wasn't used yet because that would download and set up its plugins without the user doing anything. A configuration file in `node_modules` is ignored, so leave those out of the file watcher. With the `watchFiles` feature, the server registers these files with the client instead.
- The `.gitignore` files also change which documents a configuration file formats. The server reads them again when they change, but doesn't watch them or send `dprint/configChanged` for them, so get the `dprint/fileInfo` of the open documents again when the editor sees one change, if it watches them.
- The server watches dprint's own files, which are outside the workspace and depend on the system: its settings file (ex. a domain added to `editor.trustedDomains`), its `editor-approvals.json` (ex. the user ran `dprint fmt` or allowed a process plugin in another editor window), and the global configuration file. When one changes, it resolves the configuration files that were used again and sends `dprint/configChanged` for the ones whose results changed.
- The server doesn't watch dprint's cache, so a plugin that was downloaded outside the editor (ex. by running `dprint config install` in a terminal) is only used once the configuration file is resolved again (ex. when the user approves the download, or with `dprint/reset`).
- When the server exits unexpectedly, start it again for the next request (with a delay that grows when it keeps exiting) and get `dprint/configInfo` again for each configuration file the editor extension knows about and `dprint/fileInfo` for the open documents, because the new server doesn't know them (ex. the files they were resolved from) or publish their diagnostics until they're used. The new server doesn't know what the user decided for the session (ex. a download they denied or a process plugin they allowed), so keep the session's decisions (the ones that aren't `always`, and an `always` one whose result had a `notice` because it couldn't be stored) and send them again with `dprint/approve` instead of asking whenever their approval ids are pending again (the ids are the same in the new server, and an approval of process plugins that were set up since has the id it had before in `idBeforeSetUp`), including an approval that a decision that was sent again revealed, and keep which ids were asked about, so the user isn't asked again. Don't send a decision that allows an approval that's now `denied` (ex. it was denied in another window since), and a command or the status bar item asks about what the user denied for the session again instead of sending the denial. Then get the results again like after any `dprint/approve` (see "Which documents dprint formats").
- A restart command is for forgetting what the user decided for the session, so forget the session's decisions and which ids were asked about too. So does `dprint/forgetDecisions` (for its configuration file, or every one), because sending a session's decision again would otherwise undo it. Use `dprint/reset` instead when only the plugins need to start again (ex. after a plugin stopped responding or was changed).
- A notification that changes the server's state (ex. `workspace/didChangeConfiguration`) applies to the requests sent after it.

### Which requests set up plugins

`dprint/format`, `dprint/fileInfo`, `dprint/configInfo`, `dprint/configSchema`, `dprint/approve`, `dprint/pendingApprovals`, and `dprint/forgetDecisions` resolve configuration files, which downloads and sets up their plugins that are approved (ex. downloads them from a trusted domain, or starts a process plugin the user allowed), so they take a `workDoneToken`. Getting `dprint/fileInfo` when a document is opened is enough to have the plugins ready by the time the user formats it, and `dprint/configInfo` does the same for a configuration file the editor found.

Setting up the plugins of a configuration file may take a while the first time, which is reported as progress. A format on save that has a deadline (ex. a synchronous one in Neovim) can cancel its request with `$/cancelRequest`, which doesn't stop the plugins from being set up, so a later format uses them. Don't time out a request while there's progress for it otherwise.

### Settings

The settings are in the `initializationOptions` of the `initialize` request and the `settings` of `workspace/didChangeConfiguration`. They may be at the top level or within a `dprint` property, and when there's an object in a `dprint` property, only it's used. A setting that isn't provided keeps its value, a setting that's `null` is set back to its default, and a setting that isn't known is ignored. Changing `useGlobalConfig` or the workspace folders sends `dprint/configChanged` without a `configFile`, because what the documents without a configuration file and the untitled documents are formatted with may have changed.

- `useGlobalConfig` (boolean) - Whether to format a file without a configuration file in an ancestor directory with the global configuration file. Defaults to `false`, or `true` when the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable is `1` or `true`. The `dprint/*` methods also take `globalConfig` for a document (ex. for a command that formats with the global configuration file, or a setting of a workspace folder).
- `ensureStableFormat` (boolean) - Whether to format again until the text doesn't change, which defaults to what the `DPRINT_EDITOR_STABLE_FORMAT` environment variable says.
- `verbose` (boolean) - Whether to log debug messages.

The process plugins of the global configuration file run without being approved because it's the user's own.

## Documents

Documents are identified by their uri (ex. `file:///home/user/project/file.ts`). The plugin that formats a document is chosen by its file path, so a document that isn't a file is formatted as one:

- An untitled document is formatted as a file in the first workspace folder, or the home directory when there isn't one. It's any uri with the `untitled` scheme (ex. `untitled:Untitled-1`), so an editor without its own uris for them (ex. Neovim or Emacs) uses one like `untitled:<buffer number>`. To format it in another directory (ex. the workspace folder of the active document), use the `fileUri` feature with the uri of a file there, which is the exact file it's formatted as (ex. `file:///home/user/project/Untitled-1.ts`), so its name decides which plugin formats it.
- A notebook cell is formatted as a file in the notebook's directory when the `notebookDocument` is provided, which is what the jupyter plugin does when `dprint fmt` formats a notebook. It's only formatted when the notebook would be.
- With the `fileUri` feature, a document with a `fileUri` is formatted as that file instead (ex. a `git:` document of a file in the workspace, or a document of a virtual file system that's backed by a file).

An untitled document or a notebook cell without a `fileUri` is named after its language (ex. `Untitled.ts` or `code_block.py`), so provide its `language`, which it's otherwise not formatted for (the `languageRequired` reason). A `fileUri` is used as it is, so its `language` isn't used. The name is the first of these that the configuration file formats, or otherwise the first of them: each of the language's `fileExtensions`, each of its `fileNames`, and then the file extension dprint knows for the language's `id`. Any other kind of uri isn't formatted for the `unsupportedDocument` reason (ex. a `git:` document without a `fileUri`).

dprint knows these language ids, which are VS Code's and are compared case insensitively: `bash`, `sh`, `shell`, and `shellscript` (`.sh`), `c#` and `csharp` (`.cs`), `c++` and `cpp` (`.cpp`), `clojure` (`.clj`), `coffeescript` (`.coffee`), `elixir` (`.ex`), `erlang` (`.erl`), `f#` and `fsharp` (`.fs`), `handlebars` (`.hbs`), `haskell` (`.hs`), `javascript` (`.js`), `javascriptreact` (`.jsx`), `julia` (`.jl`), `kotlin` (`.kt`), `latex` (`.tex`), `markdown` (`.md`), `nushell` (`.nu`), `ocaml` (`.ml`), `perl` (`.pl`), `powershell` (`.ps1`), `proto3` and `protobuf` (`.proto`), `python` and `python3` (`.py`), `restructuredtext` (`.rst`), `ruby` (`.rb`), `rust` (`.rs`), `terraform` (`.tf`), `typescript` (`.ts`), `typescriptreact` (`.tsx`), and `yaml` (`.yml`). Another id of only letters and digits is used as the extension (ex. `toml` or `json`), and an id with other characters isn't known (ex. `yaml.ansible`). An editor with other ids (ex. a Neovim filetype, a TextMate scope, or an Emacs mode) provides the `fileExtensions` or `fileNames` it associates with the language.

```ts
interface DocumentParams {
  textDocument: { uri: string };
  /** Needed for an untitled document or a notebook cell. */
  language?: DocumentLanguage;
  /** The notebook when the document is one of its cells, with the file to format its cells as cells of instead of its uri with the `fileUri` feature. */
  notebookDocument?: { uri: string; fileUri?: string };
  /** The file uri to format the document as instead of its uri, which is only used with the `fileUri` feature. */
  fileUri?: string;
  /** When the global config file is used when there's no config file in an ancestor directory, which is what the setting says when not provided. */
  globalConfig?: "whenNoConfigFile" | "never";
  /** The token to report the progress of the request with (ex. downloading the plugins of the configuration file). */
  workDoneToken?: string | number;
}

interface DocumentLanguage {
  /** The editor's id of the language (ex. "typescript"). */
  id: string;
  /** The file extensions the editor associates with the language, with or without the leading period (ex. ".ts" or "ts"). */
  fileExtensions?: string[];
  /** The file names the editor associates with the language (ex. "Dockerfile"). */
  fileNames?: string[];
}
```

Positions are in UTF-16 code units, which is the result's `positionEncoding`, whatever position encodings the client says it supports.

The server uses canonical paths, which have their symlinks resolved, but the uris it provides (ex. `configFile.uri`, `configFile.dirUri`, `watchUris`, the uris of diagnostics, and `dprint/configChanged`) are of the paths the editor knows the files by (ex. a workspace opened through a symlink, a Windows junction or `subst` drive, or `/tmp` on macOS being `/private/tmp`). It learns them from the workspace folders and the uris it's sent, so send the editor's own uris. A file in a directory the editor didn't tell the server about (ex. a configuration file that a symlink in the workspace points to) has its canonical path. The uris are encoded like the `url` crate encodes a file path (ex. `file:///C:/Users/user/project` on Windows), which may differ from the editor's (ex. `file:///c%3A/Users/user/project`), so compare them as file paths.

## Which documents dprint formats

An editor usually needs to know which documents an extension formats before formatting them (ex. to register as their formatter). Find the configuration files from the documents: the `configFile` of a document's `dprint/fileInfo` is the configuration file that's used for it, which may be in an ancestor directory of the workspace folder. An editor that needs to know them before a document is opened (ex. to register a formatter for their documents) may also search the workspace folders with the `configFileGlobs` and the ancestor directories of the workspace folders, leaving out what the editor excludes from searches (ex. build directories). Getting `dprint/configInfo` sets up a configuration file's plugins, so a configuration file found that way may download plugins that dprint wouldn't use for any document (ex. one in an ignored directory). Leave out `node_modules` too, whose configuration files are only used for the documents in it. A `.dprint.json` in a `.config` directory (other than the home directory's) isn't a configuration file dprint finds, so `dprint/configInfo` fails with `-32602` for it.

Get each configuration file's plugins with `dprint/configInfo`:

- `filePatterns` are globs of the documents its plugins may format, which are relative to the directory of the configuration file (`configFile.dirUri`). Register them as the documents the editor extension formats, and the cells of the notebooks they match, unless the status's `claim` is `false`.
- `otherFiles` are the other documents the configuration file formats, which a glob can't match:
  - `shebangs` - Files without an extension whose first line is one of the values, or starts with one followed by whitespace (ex. `#!/usr/bin/env node`). Register the files without an extension (ex. by checking the first line, or with every file) and get their `dprint/fileInfo`.
  - `associations` - The files of the patterns of a plugin's `associations` that can't be made relative to `configFile.dirUri` (ex. `../shared/**/*.ts`). The values are the patterns as they're written in the configuration file (relative to its directory), which are for showing the user and aren't globs to match, so treat every document like it may be formatted and get its `dprint/fileInfo`.
  - Treat a kind that isn't known like every document may be formatted, and get its `dprint/fileInfo`.
- The patterns are a superset of what's formatted, because the configuration file's `includes` and `excludes` aren't in them and a configuration file in a descendant directory may format the documents there instead. Before acting as a document's formatter (ex. when it's opened), get its `dprint/fileInfo` and only act as its formatter when its status is `ready` or its `claim` is `true`, so the editor's other formatters are used otherwise. The server says to keep claiming a document when the user needs to be told why it isn't formatted (ex. the configuration file has an error or something that isn't denied needs to be approved). An editor without a formatter registry (ex. one that runs `dprint/format` on save for every document) does the same: only format a document when the user saves it once it's claimed.
- When something needs to be approved, `plugins` and `filePatterns` are what they were the last time the plugins were set up, which is remembered across sessions, so claim those documents and ask when the user formats one (see "Approving"). Only those documents are `approvalRequired`, and another document is `notMatched` or `noPlugin`.
- When `plugins` is `null`, they aren't known (ex. a plugin that needs to be downloaded) and `filePatterns` is empty. Every document in the directory is then `approvalRequired` with `claim` set to `false`. Don't claim the documents of the configuration file, so the editor's other formatters are still used, and show what needs to be approved instead.
- When the configuration file has an error, `plugins` and `filePatterns` are the ones it had before, so a typo doesn't change what the editor formats with it.

The globs use `/` to separate path segments and are relative without a leading `/` or `./`. A `**` segment matches any number of segments (including none), `*` matches any characters within a segment and `?` one character, which includes names that start with a period (ex. `**/*.json` matches `.config/.eslintrc.json`, like picomatch's `dot: true`), and `[...]` matches one of the characters of the class, which may have ranges (ex. `[a-z]`) and be negated with a leading `!` (ex. `[!.]`), and doesn't match `/`. There aren't braces or negated patterns. A character that's otherwise special (`*`, `?`, `[`, `{`, or `}`) is in a class of its own when it's matched literally (ex. `[*]`), and file extensions and names are matched case insensitively with a class of both cases (ex. `**/*.[tT][sS]`). These are VS Code's globs. Check the options of another glob library (ex. picomatch and minimatch skip names that start with a period by default, and Python's `fnmatch` has `*` match `/`).

The results stay valid until the server says otherwise, so get them again only when:

- The server sends `dprint/configChanged` for the configuration file, or without a `configFile`. It sends it whenever what a configuration file formats may have changed: when one the editor said was created, changed, or deleted, when a file it was resolved from changed, when resolving it for a request has a different result (ex. a remote configuration file it extends changed), when what needed to be approved was approved or denied (including outside the editor, which the server notices in dprint's own files), and without a `configFile` when `useGlobalConfig`, the workspace folders, or whether there's a global configuration file changed.
- `dprint/approve`, `dprint/forgetDecisions`, or `dprint/reset` responded. A `dprint/configChanged` may also be sent for the same change, so getting the results again once for both is enough (ex. by waiting a moment).
- The server was started again.

When it's sent, get `dprint/configInfo` for the configuration file and `dprint/fileInfo` for the open documents it may be used for, because a change to its `includes` or `excludes` changes whether they're formatted. Those are the documents in its `dirUri`, the documents its `associations` may match (see `otherFiles`), every document for the one the server was started with (`kind: "cli"`), and the documents without a configuration file when it was created. Without a `configFile`, get them for every configuration file and open document. The results of `dprint/format` say whether to claim a document (`claim`, including a `failed` result's) and `dprint/approve` says whether its configuration file is used (`status`), which replace what was known before for them.

A configuration file is identified by its `uri` and the directory it's used for (`dirUri`), which the server always provides together. The global configuration file has results for each directory it's used for (ex. each drive). Send the `dirUri` with the `uri` when a method takes a configuration file and it's known (ex. from a result), which is needed for the global configuration file and the one the server was started with, and isn't for one the editor found (a configuration file in the workspace is used for its own directory). Keep what's known about one by both, and treat a `dprint/configChanged` whose `configFile` doesn't have a `dirUri` like it's for each one.

## Approving

Opening a project isn't the same as choosing to run what its configuration file says, so the server only uses a configuration file once the user approves what it needs:

- **Downloads** - A download sends a request to the domain it's from, which a configuration file could use to send information to a server of its choosing (ex. that the project was opened and from where). To limit that, the server only downloads plugins and the configuration files that a configuration file extends from these trusted domains (and their subdomains) without the user approving it: `registry.npmjs.org`, `plugins.dprint.dev`, `github.com`, `githubusercontent.com`, and the ones the user added to the `editor.trustedDomains` setting in dprint's settings file. This includes each redirect, and reading a file on another machine (ex. a UNC path on Windows, or a path in `/net` on macOS, which the system connects to when it's read) is approved like a download, unless it's on the same share as the configuration file. A download is also only sent through a proxy that an `.npmrc` file sets for npm packages (`https-proxy` or `proxy`) once the user approves it, unless the file is the user's own `~/.npmrc`. That includes the files in the configuration file's directory and every ancestor directory, because they may be the project's, and the project would otherwise choose a server that receives every download along with the credentials in the setting, which may have the values of the user's environment variables (ex. `https://${GITHUB_TOKEN}@proxy.example`). Until then, a download that would go through it needs approval instead of going another way. Each proxy is its own approval of the `downloads` kind, so its message and decision are only about it and not hidden among the urls (ex. when a long message is cut off). What's in dprint's cache doesn't need to be downloaded (ex. from running `dprint fmt` or `dprint config install`), and neither does a plugin that's a file (ex. in `node_modules`). Allowing a download allows each of its exact urls for the rest of the session, including for the other configuration files that need them (the url of an npm registry allows every package from it), which are then used too. Allowing a proxy allows the text of its setting in that `.npmrc` file, so the user is asked again when the setting changes. A decision that isn't `always` is only for the server that received it, so another editor window asks again.
- **Process plugins** - They aren't sandboxed, so running them needs to be approved for the configuration file with each plugin's configuration, unless the user set `editor.allowProcessPlugins` to `true` in dprint's settings file (see [the language server docs](../website/src/lsp.md)). That setting isn't used when the settings file can't be read or has a property in `editor` that the server doesn't know (ex. a setting of a newer version of dprint), which also has the server only trust the default domains. Running dprint with the configuration file (`dprint fmt`, `dprint check`, or `dprint config install`) approves them because the user ran it.

Nothing is formatted with the configuration file until then, not even by its Wasm plugins, because formatting only some files or formatting them with other plugins would format differently than the CLI. The results of `dprint/format`, `dprint/fileInfo`, and `dprint/configInfo` then have the `approvalRequired` reason and what needs to be approved (see "Which documents dprint formats" for which documents). The `approvals` of any result are what to ask about, whatever its reason, but only ask when the user wants to format with the configuration file (see below):

```ts
interface PendingApproval {
  /**
   * Identifies exactly what's approved, which changes when what's shown does (ex. the configuration of a process plugin) and is the
   * same in another server until dprint changes how the ids are made, which is rare. Only send a decision for the id that was shown
   * to the user.
   */
  id: string;
  /** Treat a kind that isn't known like "processPlugins" with its `message`. */
  kind: "downloads" | "processPlugins" | (string & {});
  /** A short description of what's approved (ex. for a status bar item or the heading of a prompt). */
  title: string;
  /** The configuration file that needs it, which is what the user approves for. */
  configFile: ConfigFileInfo;
  /** What's downloaded or run. */
  items: ApprovalItem[];
  /** The decisions the user can make, in the order to show them. */
  decisions: ApprovalDecisionOption[];
  /** Whether the user denied it, which means they shouldn't be asked unless they ask for it (ex. a command). */
  denied: boolean;
  /** What the user decided before, which is "deny" or "alwaysDeny" when it's denied. */
  previousDecision?: ApprovalDecision;
  /**
   * The message to show the user when asking, which says what's approved and why. It has everything the user needs to see
   * to decide apart from the configuration of the items (ex. the registry of an npm plugin or the `.npmrc` file of a proxy),
   * so a client that can't show the items shows it.
   */
  message: string;
  /**
   * The id the approval had before its process plugins were set up (ex. in a server that was started before), which is
   * for the same trusted configuration. Send a decision the user made for the session for that id again with `id`.
   */
  idBeforeSetUp?: string;
  /**
   * The trusted configuration of the configuration file, including what it extends, which the process plugins that weren't set up
   * before run with once they're allowed (what each one runs with isn't known until it's set up).
   */
  trustedConfiguration?: object;
}

interface ApprovalItem {
  /**
   * The url, path, or npm specifier (ex. of a plugin with its checksum). For an `npmrcProxy`, it's the text of the setting before its
   * environment variables are replaced (ex. `https://${GITHUB_TOKEN}@proxy.example`), which shows what would be sent to the proxy.
   */
  specifier: string;
  /**
   * A config file is one that a config file extends, and an npmrcProxy is a proxy that an `.npmrc` file sets, which downloads are sent
   * through once it's allowed. Show a kind that isn't known by its specifier.
   */
  kind: "plugin" | "config" | "npmrcProxy" | (string & {});
  /**
   * The npm registry an npm plugin is from when it isn't the default one (`https://registry.npmjs.org/`), which an `.npmrc` file
   * can set (ex. the project's for a scope). Show it with the specifier because the specifier doesn't say where the plugin is from.
   */
  registry?: string;
  /** The uri of the `.npmrc` file of an `npmrcProxy` as the editor knows it, which is shown with it (ex. with a way to open it). */
  npmrcUri?: string;
  /** The plugin when it was set up before. */
  plugin?: FilePlugin;
  /** The configuration a process plugin that was set up before runs with (ex. the commands of dprint-plugin-exec). */
  configuration?: unknown;
}

interface ApprovalDecisionOption {
  id: ApprovalDecision;
  /** What to show the user. */
  label: string;
  /** Whether it allows what's approved, which is otherwise denied. */
  allows: boolean;
  /** Whether it's remembered after the server stops, which is otherwise only for the session. */
  always: boolean;
  /** What it does (ex. for how long and for what), which is shown with it (ex. as a tooltip, or in the message when the editor can't show one). */
  description: string;
}

/** Show a decision that isn't known by its option's `label` and `description`. */
type ApprovalDecision = "allow" | "alwaysAllow" | "deny" | "alwaysDeny" | (string & {});
```

Ask about the approvals of a result when the user wants to format with the configuration file (ex. a format on save or a command), and not when a project is opened, because the editor makes requests without the user doing anything. A format the user didn't run as a command only asks when the result's `claim` is `true`, or when the user chose the editor extension as the formatter of the document's language (ex. VS Code's `editor.defaultFormatter`). Show the approval's `message` instead of the `message` of the `approvalRequired` result, ask the same about a kind that isn't known, and only offer the approval's `decisions`. Show the configuration file (and that it's outside the workspace when its `inWorkspace` is `false`) and what will be downloaded or run so the user knows what they're approving, apart from the `message` because it may be cut off when it's long: each item's `specifier` (with the `npmrcUri` of an `npmrcProxy`), each item's `registry` when it has one, each item's `configuration`, and the `trustedConfiguration` when there is one, which is what a process plugin that wasn't set up before runs with. It includes what the configuration file extends, so opening the configuration file isn't enough to see it. It may be large, so it can be collapsed (ex. behind a "Show Configuration" button) as long as the user can see it before deciding. Also show that the configuration file needs approval somewhere that doesn't interrupt the user (ex. a status bar item that asks when it's clicked), because a denied approval isn't asked about again and the editor doesn't act as the formatter of its documents. `dprint/pendingApprovals` has what needs to be approved for that: get it again when the results are gotten again (see "Which documents dprint formats"), and close a prompt whose approval isn't pending anymore (ex. it was decided in another window).

The editor formats without the user doing anything in between (ex. a "save all" of many documents), so don't let asking turn into a storm of prompts:

- Show one prompt for an approval `id` at a time, which every format waiting on it shares.
- A format the user didn't run as a command asks about an `id` at most once, even when the prompt is closed without deciding, because that leaves it pending, and also after the server is started again. A command or the status bar item asks again, including about what was denied.
- Don't make a save wait on the prompt. Once it's approved, format the document's current text again, which may have changed in the meantime, and only apply the edits when the document didn't change while it was formatted. Save it again when it was saved before and is unchanged since.

Several may need to be approved at once (ex. downloading a plugin and running a process plugin). Ask about them one at a time, or show them together and send the decisions one at a time, sending the next only while its `id` is still in the previous result. Send what the user decided with `dprint/approve` and take what to ask about next from its result, which is what needs to be approved now (ex. a configuration file that a downloaded one extends or the process plugins of a downloaded configuration file). Stop when the result has nothing that isn't denied, and format the document again when its `status` is `ready`.

`-32803` means what was approved changed before the decision was received (ex. the configuration file changed), so the decision wasn't used. Its `data` has what needs to be approved now for the configuration file and whether it's used (`current`) when they're known, which is handled like a result of `dprint/approve`. For a decision whose option's `allows` is `false`, don't ask again right away: send the same denial for the approval of the same `kind` and `configFile` that replaced it, if there is one, it has that decision, and its items are some of the ones of the approval that was denied (so a plugin added since isn't denied without the user seeing it), and never send an allowing decision that the user didn't make for what's shown. When the request is cancelled or fails otherwise, or `-32803` doesn't have `current`, the decision may still have been used, so get what's pending for the configuration file with `dprint/pendingApprovals` (with its `configFile`) and continue with its result.

The decisions that are `always` are stored in an `editor-approvals.json` file next to dprint's settings file (`files.approvalsUri`), which the server notices when it changes (see "Lifecycle"). Every version of dprint that's installed uses the same file (ex. a project's npm dprint and a global one), so a decision stored by one is used by the others that can read it, and an older version keeps what a newer one stored in it that it doesn't know. `dprint/decisions` lists what the user decided (ex. for a command that shows them), and `dprint/forgetDecisions` forgets what the user decided for a configuration file, or for every configuration file, so they're asked again (ex. a command to undo allowing a process plugin).

## Methods

The server responds with an error to a request that can't be handled, which has its message and a code:

- `-32602` (InvalidParams) - The params can't be handled (ex. an `overrideConfig` property that can't be overridden, a decision the approval doesn't have, or a `configFile` that isn't a configuration file).
- `-32803` (RequestFailed) - What `dprint/approve` was sent for isn't pending anymore because what needs to be approved changed (see "Approving"). The error's `data` is `{ kind: "approvalChanged", current?: ConfigApprovals }`, where `current` is for the configuration file the approval was for and is missing when that isn't known (ex. the id isn't of an approval the server provided). Recognize it by its `data.kind`, and treat it like `-32603` without it.
- `-32800` (RequestCancelled) - The request was cancelled with `$/cancelRequest`, which isn't shown to the user.
- `-32601` (MethodNotFound) - The method isn't known (ex. one that's newer than the server), or the server handles the client like a language client.
- `-32603` (InternalError) - Anything else (ex. a configuration file couldn't be read), which doesn't mean the server stopped. Treat a code that isn't known like this.

The `data` of an error is an object with a `kind`, where a kind that isn't known is ignored. The methods that take params take an object that may have fields added later, so a method without params takes `{}`, and every result is an object.

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
   * something the user didn't approve. `indentWidth` is a positive integer.
   */
  overrideConfig?: { indentWidth?: number; useTabs?: boolean };
}

interface FormatResponse {
  /** The configuration file that was used for the document, which is `null` when there isn't one. */
  configFile: ConfigFileInfo | null;
  result: FormatResult;
}

type FormatResult =
  /** The edits of the text that was sent, which don't overlap and are in order. It's empty when the text is already formatted. */
  | { kind: "formatted"; edits: TextEdit[]; notice?: Notice }
  | ({ kind: "notFormatted" } & NotFormatted)
  /** A plugin couldn't format it (ex. a syntax error) or failed (ex. it panicked), which says the plugin when it's known. */
  | {
    kind: "failed";
    reason: "formatError" | "pluginError" | (string & {});
    message: string;
    show: ShowPolicy;
    severity: Severity;
    /** Whether to keep acting as the document's formatter, which is `true` for a failure for now (ex. a later failure may not be claimed). */
    claim: boolean;
    actions?: Action[];
    plugin?: FilePlugin;
  };

/** Something to tell the user about the result of a request (ex. a warning about a document that was formatted, or a decision that couldn't be stored). Show it like its `show` and `severity` say (see `dprint/format`). */
interface Notice {
  message: string;
  show: ShowPolicy;
  severity: Severity;
  actions?: Action[];
}

/** Something the user can do about a message, which is offered with it. Leave out a kind that isn't known. */
type Action =
  /** Opens the file in the editor (ex. the configuration file that has an error). */
  | { kind: "openUri"; label: string; uri: string }
  | { kind: string & {}; label: string };

/** Why a document isn't formatted, which is the same for each method. */
interface NotFormatted {
  reason: NotFormattedReason;
  /** The message to show the user. */
  message: string;
  /** When to show the message. */
  show: ShowPolicy;
  /** How to show the message. */
  severity: Severity;
  /** Whether to keep acting as the document's formatter so the user is told why it isn't formatted, which otherwise leaves it to the editor's other formatters. */
  claim: boolean;
  /** What needs to be approved to format it (see "Approving"). */
  approvals?: PendingApproval[];
  /** What the user can do about it. */
  actions?: Action[];
}

/**
 * When to show a message, which is only for a request the user made (ex. a format on save or a command) and not one the
 * editor made without the user (ex. `dprint/fileInfo` when a document is opened). Treat a value that isn't known like "command".
 */
type ShowPolicy =
  /** Each time (ex. a plugin failed). */
  | "always"
  /** Only when the user ran a command (ex. a syntax error, which a format on save would otherwise show each time). */
  | "command"
  /** Once for each configuration file until the message changes or the server sends `dprint/configChanged` for it (ex. an error in the configuration file). */
  | "oncePerConfigFile"
  | (string & {});

/** How to show a message (ex. an error notification). Treat a value that isn't known like "warning". */
type Severity = "error" | "warning" | "info" | (string & {});

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
  /** The config file that was asked about doesn't exist (ex. it was deleted). */
  | "configFileMissing"
  | (string & {});
```

Each range is formatted in order, and overlapping ranges are merged. A plugin that supports range formatting decides what to format around a range, and the range is expanded to whole lines for a plugin that doesn't say. The edits are of the text that was sent, and the document's line endings are kept (a text without a line break gets the configuration's line endings).

Show the message of a result, and of a `notice`, like its `show` and `severity` say, with its `actions`. A format that the editor can't tell apart from one the user didn't run as a command (ex. a formatter that's also run on save) isn't a command. For `oncePerConfigFile`, the configuration file is the result's `configFile` (its `uri` and `dirUri`), or the result's `message` alone when it's `null`. The server doesn't show them to an editor extension's user. Treat a kind that isn't known like a result that isn't formatted with the fields it has (ex. `claim`, `show`, and `message`), or like `failed` that's only shown when the user ran a command when it doesn't have them.

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

/** Treat a kind that isn't known like "notFormatted" with the fields it has, which doesn't claim the document without `claim`. */
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
   * The directory whose files it formats, which is the directory it's in unless that's a `.config` directory (other than the home directory's),
   * and the directory it's used for when it's the global configuration file (ex. the root of a drive). It's the
   * directory its patterns are relative to for a remote or npm configuration file.
   */
  dirUri: string;
  /**
   * How it was chosen: found in an ancestor directory, the global configuration file, or the one the server was started
   * with (`dprint lsp --config <path>`). Treat a kind that isn't known like "local".
   */
  kind: "local" | "global" | "cli" | (string & {});
  /** Whether it's in a workspace folder, which the global configuration file and one in an ancestor directory of the workspace folders aren't. */
  inWorkspace: boolean;
}
```

### `dprint/configInfo` (request)

Gets the plugins of a configuration file.

```ts
/** Either the configuration file itself or a document it's used for, and not both. */
type ConfigParams = { configFile: ConfigFileIdentifier; workDoneToken?: string | number } | DocumentParams;

interface ConfigFileIdentifier {
  uri: string;
  /**
   * The directory it's used for (its `dirUri`), which is the root of the server's current directory for the global configuration
   * file when not provided, and the directory to find the configuration file the server was started with from (`kind: "cli"`).
   */
  dirUri?: string;
}

interface ConfigInfoResult {
  configFile: ConfigFileInfo | null;
  /** `null` when they aren't known (see "Which documents dprint formats"). */
  plugins: PluginInfo[] | null;
  /** The documents the plugins may format. */
  filePatterns: FilePattern[];
  /** The other documents it formats (see "Which documents dprint formats"). */
  otherFiles: { kind: "shebangs" | "associations" | (string & {}); values: string[] }[];
  /** The local files besides the configuration file that it was resolved from (ex. a configuration file it extends), which the editor watches too. */
  watchUris: string[];
  status: FormatStatus;
}

interface FilePattern {
  /** A glob relative to `configFile.dirUri` (see "Which documents dprint formats"). */
  glob: string;
  /** The config keys of the plugins that format the documents. */
  configKeys: string[];
}

interface PluginInfo extends FilePlugin {
  /** The file extensions it formats without the leading period. */
  fileExtensions: string[];
  /** The file names it formats regardless of their extension. */
  fileNames: string[];
  /** Whether it formats a file in addition to the plugin that formats it instead of on its own (ex. sorting the keys of a package.json). */
  additive: boolean;
  /** Whether it decides what to format around a range, formats the whole lines of the range, or doesn't format a range. Treat a value that isn't known like "unsupported". */
  rangeFormatting: "supported" | "wholeLines" | "unsupported" | (string & {});
  helpUrl: string;
  /** Where its newer version is said to be. */
  updateUrl?: string;
}
```

For example, with the configuration file:

```json
{ "configFile": { "uri": "file:///home/user/project/dprint.json" } }
```

Or with a document it's used for:

```json
{ "textDocument": { "uri": "file:///home/user/project/src/file.ts" } }
```

Providing both or neither fails with `-32602`. With `configFile`, it's the information of that configuration file, which needs the name of a configuration file, to be the global configuration file, or to be the one the server was started with (`dprint lsp --config <path>`), which is the only one when there's one. Its status is `configFileMissing` when it doesn't exist. Getting it sets up the plugins that are approved (ex. downloads them), so get it once a configuration file is found so the first format doesn't wait on that.

The status is for the configuration file, so it's `ready` or why none of its documents are formatted (ex. `configError`, `approvalRequired`, or `configFileMissing`), while `dprint/fileInfo` has the reasons for a document (ex. `notMatched`).

### `dprint/configSchema` (request)

Gets the JSON schema (draft 7) of a configuration file (`ConfigParams`), which has the schemas of its plugins, so the editor can provide completions and validation for it. A plugin's schema is only downloaded from a trusted domain.

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

/** The configuration file the approval was for and what needs to be approved for it now. */
interface ApproveResult extends ConfigApprovals {
  /** What to tell the user about the decision (ex. it couldn't be stored, so it's only used until the server stops). */
  notice?: Notice;
}

/**
 * What needs to be approved for a configuration file now and whether it's used, which is the same for `dprint/approve`,
 * `dprint/pendingApprovals` with a `configFile`, and the `data` of `-32803`. Handle each of them the same way.
 */
interface ConfigApprovals {
  /** The configuration file, which is `null` when it can't be resolved anymore (ex. it was deleted). */
  configFile: ConfigFileInfo | null;
  /**
   * What needs to be approved now for the configuration file, which is empty when nothing does. After a decision, it has
   * what was denied when the decision was to deny it, and otherwise it may have something else that needs to be approved.
   */
  approvals: PendingApproval[];
  /**
   * Whether the configuration file is used now, which isn't "ready" when it has an error or was deleted, even when nothing needs to
   * be approved. It's for the configuration file, so get the `dprint/fileInfo` of a document to know whether it's formatted.
   */
  status: FormatStatus;
}
```

The decision is used once the server receives it, even when the request is cancelled. The approval is for its `configFile`, even when a document it was asked about for uses another configuration file now. Allowing it downloads and sets up what was allowed, and then the configuration file is resolved again for the result. It fails with `-32803` when nothing is pending with the id.

### `dprint/pendingApprovals` (request)

Gets what needs to be approved, which includes what the user denied (ex. for a status bar item or a command that asks about them).

```ts
interface PendingApprovalsParams {
  /** The configuration file to get them for, which is every configuration file that was used when not provided. */
  configFile?: ConfigFileIdentifier;
  workDoneToken?: string | number;
}

/** It's a `ConfigApprovals` (with `configFile` and `status`) exactly when the params have a `configFile`. */
type PendingApprovalsResult = { approvals: PendingApproval[] } | ConfigApprovals;
```

### `dprint/forgetDecisions` (request)

Forgets what the user decided about the approvals of a configuration file, or of every configuration file, including the decisions that are `always` and the ones from running dprint (ex. `dprint fmt`), so the user is asked about them again. A download that was allowed stays allowed for the session, and a plugin that's in dprint's cache doesn't need to be downloaded again. It responds once the configuration files were resolved again, and sends `dprint/configChanged` for the ones whose results changed.

```ts
interface ForgetDecisionsParams {
  /** The configuration file to forget the decisions of, which is every configuration file when not provided. */
  configFile?: ConfigFileIdentifier;
  workDoneToken?: string | number;
}

interface ForgetDecisionsResult {
  /** The decisions that were forgotten. */
  decisions: Decision[];
  /** What to tell the user about forgetting them (ex. the stored decisions couldn't be forgotten). */
  notice?: Notice;
}
```

### `dprint/decisions` (request)

Gets what the user decided about the approvals of a configuration file, or of every configuration file (ex. for a command that shows them). A process plugin of a configuration file has the decision that's used for it, which is the most recent one. A decision that allows it may be for an earlier configuration of the plugin, which is then asked about again when it's used instead of an older decision being used, while a decision that denies it is for any configuration.

```ts
interface DecisionsParams {
  /** The configuration file to get the decisions of, which is every configuration file when not provided. */
  configFile?: ConfigFileIdentifier;
}

interface DecisionsResult {
  decisions: Decision[];
  /** Whether the settings file allows every process plugin without asking (`editor.allowProcessPlugins`). */
  allowProcessPlugins: boolean;
}

interface Decision {
  /** Show a kind that isn't known with its `specifiers`. */
  kind: "downloads" | "processPlugins" | (string & {});
  /**
   * The configuration file it's for, which can be sent to `dprint/forgetDecisions`. It's missing for a download that's allowed
   * for every configuration file, and its `dirUri` is missing when it's for every directory the configuration file is used
   * for (ex. the global configuration file).
   */
  configFile?: ConfigFileIdentifier;
  /** The urls, paths, or specifiers it's for, like the `specifier` of an approval's items (ex. the process plugins, or the setting of an `.npmrc` proxy). */
  specifiers: string[];
  /**
   * What it's for, like an approval's items in the order of the `specifiers`, which the server provides for downloads so a proxy can be
   * told apart from a url and shown with its file (an `npmrcProxy` with its `npmrcUri`). Show the `specifiers` when it's missing.
   */
  items?: ApprovalItem[];
  allows: boolean;
  /** Whether it's remembered after the server stops, which includes running dprint with the configuration file (ex. `dprint fmt`). */
  always: boolean;
}
```

### `dprint/reset` (request)

Forgets the configuration files and stops the plugins, which are set up again when they're next used (ex. after a plugin stopped responding or was changed). Unlike starting a new server, what the user decided for the session is kept. It takes `{}` and its result is `{}`. Get the plugins of the configuration files again afterwards because a plugin that changed may format other files.

### `dprint/configChanged` (notification from the server)

```ts
interface ConfigChangedParams {
  /** The configuration file, which is every configuration file when it's missing. */
  configFile?: ConfigFileIdentifier;
  /** How it changed. Treat a change that isn't known like "changed". */
  change?: "created" | "changed" | "deleted" | (string & {});
  /** Whether only its schema changed (ex. a plugin's schema downloaded), so get the schema again but not the plugins. */
  schemaOnly?: boolean;
}
```

See "Which documents dprint formats". Ignore a configuration file the editor extension doesn't know about unless its `change` is `created`.

### Language server protocol methods

The server also uses these, which an editor extension should handle:

- `window/logMessage` - What the server logs, which isn't written to stderr. Stderr only has what isn't (ex. a crash).
- `window/showMessage` - Something to show the user that isn't in the result of a request. The server doesn't send it to an editor extension now (what goes wrong with a request is in its result, ex. a `notice`), but a later version may.
- `window/workDoneProgress/create` and `$/progress` - What's being done that may take a while (ex. downloading a plugin), when the client supports it. What's done while a request with a `workDoneToken` is handled is reported on its token, which is the total of everything that's being done (ex. a download that started before the request), and what starts then is reported on the server's own token once the request is done.
- `textDocument/publishDiagnostics` - The problems with a configuration file (ex. an unknown property), which replace the ones published before. A problem without a known position is at the start of the file.
- `$/cancelRequest` - Cancels a request (ex. a format the editor doesn't need anymore).
- `workspace/didChangeWatchedFiles` - See "Lifecycle".
- `client/registerCapability` and `client/unregisterCapability` - With the `watchFiles` feature, the files the server needs to be told about.

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
- A decision isn't remembered by url or domain beyond the session, and allowing a download only allows its urls, because a broader scope (ex. trusting a domain) is the settings file's `editor.trustedDomains`, which the user edits on purpose.
-->
