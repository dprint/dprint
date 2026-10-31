# Developing an Editor Extension

An editor extension formats documents with the `dprint lsp` command, which is a language server. A language client formats documents with the text document methods of the language server protocol (see [the language server docs](../website/src/lsp.md)). An editor extension uses the `dprint/*` methods described here instead, which are only sent a document's text when formatting it, so the server doesn't keep the documents. They also say which documents dprint formats and let the editor extension ask the user what needs to be approved. An editor whose extensions can't send custom requests or handle custom notifications (ex. Zed or Helix) uses `dprint lsp` as a language client instead (see [the language server docs](../website/src/lsp.md)).

The `dprint/*` methods were introduced in dprint 0.65. Editor extensions for older versions used `dprint editor-service` (see [schema version 5](./editor-extension-development-schema-5.md)), which formats the files of a configuration file that the editor found.

An editor extension usually does the following, which the sections below describe:

1. Start `dprint lsp`, send `initialize` with the `dprint` capability and a session id of its own for the server (`sessionId`), check the `version` of the server's capability, and send `initialized` (see "Starting the server"). When the server exits unexpectedly, start it again with the same session id once its process exited, which keeps what the user decided for the session (see "Lifecycle").
2. Watch the configuration files (`configFileGlobs`) and the files they were resolved from (`watchUris`) with watchers of their directories that don't follow symlinks, and send `workspace/didChangeWatchedFiles` when they change (see "Lifecycle").
3. When a document is opened, get its `dprint/fileInfo`, which says whether dprint formats it (`status`) or the editor extension should act as its formatter anyway (`claim`), and which configuration file is used for it (`configFile`). Get `dprint/configInfo` for that configuration file, which says which other documents it may format (see "Which documents dprint formats").
4. Format a document with `dprint/format`. When something needs to be approved first (the `approvalRequired` reason), ask the user and send their decision with `dprint/approve` (see "Approving").
5. When the server sends `dprint/configChanged`, get the `dprint/configInfo` and `dprint/fileInfo` results again.
6. Show the `message` of a result like its `show` and `severity` say (see `dprint/format`).

## Starting the server

Start one `dprint lsp` process for each `dprint` executable the editor uses (usually one for the whole editor window, not one for each workspace folder), each with its own `sessionId`, and send the `initialize` request with the `dprint` experimental capability:

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

Then send the `initialized` notification, which is when the server registers what it needs from the client (ex. the files to watch with the `watchFiles` feature).

`sessionId` identifies the server's session, which is unique to the server (and ids that are only different in case are the same session on a case insensitive file system, because it names the session's file, so make them random, ex. a UUID): make a new one for each server the editor extension starts (ex. a random UUID for each `dprint` executable of each editor window, so the server of a project's npm dprint and the server of the global one in the same window each have their own), and only use it again to start that same server again once its process exited without its session ending (ex. it crashed, or the editor extension killed it because it stopped responding), and not after a `shutdown` and `exit`, which end the session, or for the restart command (see "Lifecycle"). The server keeps what the user decided for the session (ex. a download they denied or a process plugin they allowed for the session) in a file for it in dprint's cache directory, so a server that's started again with the same id uses what the user decided like the server that exited (see "Lifecycle"). Two servers that are running with the same id would overwrite each other's decisions (ex. one would bring back what the user allowed in it after they denied it in the other), so a server whose id another server that's running uses (ex. the server it replaces hasn't exited yet), or whose session's lock can't be taken (ex. the file system of dprint's cache directory doesn't support locks), doesn't use the file, which is logged: it neither uses what the other server kept nor writes to it, and the results of `dprint/approve` and `dprint/forgetDecisions` have a `notice` that the session's decisions won't be kept if dprint restarts. It keeps trying to take the file over (every second, and whenever its decisions change), and once the other server exited, it writes its own decisions to it right away and keeps them from then on. Until then, the file still has what the other server kept, so when this server exits before it takes the file over, a server started again for the session uses what the other server kept instead of what the user decided in this one (ex. an allow the user denied in this server). So start a server again for a session only once the previous process exited (ex. kill it and wait for it to exit). It's 16 to 128 ASCII letters, digits, underscores (`_`), and hyphens (`-`). One that isn't valid is ignored, which is logged, and without one, what the user decided for the session is lost when the server exits.

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
      "configFileGlobs": ["**/dprint.json", "**/dprint.jsonc", "**/.dprint.json", "**/.dprint.jsonc"],
      "configFileNames": ["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"],
      "dotConfigFileNames": ["dprint.json", "dprint.jsonc"]
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
  /**
   * The id of the server's session, which is unique to the server (ex. a random UUID for each `dprint` executable of each editor window)
   * and keeps what the user decided for the session when the same server is started again with it once its process exited without the
   * session ending (ex. it crashed or was killed), and not for the restart command, which is a new session (see "Lifecycle"). It's 16 to
   * 128 of `A-Z`, `a-z`, `0-9`, `_`, and `-`.
   */
  sessionId?: string;
}

interface ServerCapability {
  /** The version of the `dprint/*` methods the server uses, which is missing when it doesn't support any of the client's. */
  version?: number;
  /** The versions the server supports. */
  versions: number[];
  /** The features it uses of the client's, which is missing when it doesn't use any (`true` uses none of the options the client sent). */
  features?: { [name: string]: true | object };
  /** Where dprint's own files are, which is missing without a `version`. */
  files?: { settingsUri?: string; approvalsUri?: string; globalConfigUri?: string };
  /** The configuration files to watch in the workspace folders, which is missing without a `version`. */
  configFileGlobs?: string[];
  /**
   * The names of the configuration files that are looked up in a directory (ex. in the ancestor directories of the workspace
   * folders), which may have names added later, are empty for `dprint lsp --config`, and are missing without a `version`. A
   * case insensitive file system (Windows and macOS by default) finds them in any case, which the server does too.
   */
  configFileNames?: string[];
  /**
   * The names of the configuration files that are looked up in the `.config` directory of a directory, like
   * `configFileNames`. The home directory's `.config` is an ordinary directory, which isn't looked in for them.
   */
  dotConfigFileNames?: string[];
  /** What to tell the user when there isn't a `version`. */
  message?: string;
}
```

`files` are where dprint's own files are, so the editor extension can offer commands that open them (ex. to add a domain to `editor.trustedDomains` in the settings file, or to edit the global configuration file). A file is left out when its directory can't be found, and `globalConfigUri` is where the global configuration file would be created when it doesn't exist, so a command that opens it may offer to create it. Use `dprint/forgetDecisions` to forget what the user decided instead of changing the approvals file.

`configFileGlobs` are the configuration files to watch in the workspace folders (see "Lifecycle"), which may have names added later. They're empty when the server was started with a configuration file (`dprint lsp --config <path>`), which is the only one, so another configuration file in the workspace isn't one to the server (ex. `dprint/configInfo` fails with `-32602` for it).

One server formats the files of a whole workspace, and also the files outside the workspace folders (ex. a file opened on its own). For each file, the server finds the configuration file in an ancestor directory, or uses the global configuration file when that's allowed. The workspace folders (or the `rootUri` without them) are where untitled documents are formatted (see "Documents"), whether a configuration file is in the workspace (`inWorkspace`), and how the editor knows the paths of the files (see "Documents"). An editor extension built on a library that starts a server for each project (ex. eglot or lsp-mode) works too, but what the user decided for the session and the plugins that were set up aren't shared between the servers, and each of them needs its own `sessionId`. Keep one for each project root (ex. a table keyed by Neovim's `root_dir`), use it again when the library starts that server again after it exited unexpectedly (ex. eglot's `eglot-autoreconnect`, or a `before_init` that looks the id up rather than making one), and only replace it for a restart command. A library's restart may stop the server without `shutdown` (ex. Neovim's `Client:stop(true)`, or a fallback to it when shutting down takes too long), which keeps its session, so a `before_init` lookup can't tell it apart from a crash. Clear the entry in the library's restart hook (or in the editor extension's own restart command) so the next start makes a new id, and keep it only after an exit the client didn't cause, which is told by what the client did rather than by the exit code (ex. a flag the editor extension sets before it stops or kills the server itself, or before it calls the library's restart, which `on_exit` checks): a server that's killed exits with code 1 on Windows, and one that crashes by a signal (ex. a stack overflow) exits with code 0 and the signal on Unix. When only `on_exit` is known, treat an exit with a signal like one the client caused, which only starts a new session after such a crash. A new `sessionId` is the only way to start a session over: `dprint/forgetDecisions` without params isn't, because it forgets every stored decision of every configuration file (including the ones from running dprint) and keeps the downloads the session allowed. Run it in the first workspace folder, or the home directory without one, because the current directory is where the global configuration file is used for when a `dirUri` isn't provided. The configuration is read again for each request, so an edited configuration file is used for the next request without restarting the server. Specifying `dprint lsp --config <path>` uses that configuration file for every file instead.

With the `dprint` capability, the server doesn't sync documents, register the text document methods, or provide completions for configuration files (the configuration schema method provides what's needed for that). Without a `version`, the `dprint/*` methods fail with `-32601`.

### Security

What dprint guarantees about reaching another machine (ex. a share that a symlink in a repository points to, which the system connects to when it's read, sending it information such as the user's credentials on Windows) has two parts:

- **Guaranteed:** dprint never contacts another machine before the user allows it through anything it does on its own: reading, looking up (ex. `stat`), canonicalizing, watching, and looking for configuration files. That's the server, and what an editor extension does without the user asking (ex. watching the files the server provides, or getting the results of the open documents), which follows the rules below.
- **Best effort:** the user opening a file themselves (ex. clicking a diagnostic or an "Open Configuration File" action) is the editor's own handling of files, like its explorer, which follows symlinks too. dprint makes a best effort there: the server takes back the diagnostics it knows are stale, and an editor extension checks the latest result it has before opening a configuration file (see `ConfigFileInfo.onAnotherMachine`). That isn't a guarantee across later changes to the repository, because the editor itself follows those symlinks.

The editor extension decides what the server is started with, so a project can't choose it:

- Use the `dprint` executable the user configured, or otherwise the one on the path. Find the one on the path yourself and start it by its absolute path, only looking in the absolute directories of the path, because starting a bare `dprint` may run one in the current directory first (ex. Node's `child_process` and Neovim on Windows look for a `dprint.exe` in the current directory, which is the workspace folder). Start the `dprint` executable itself rather than a shell, a batch file, or a wrapper that starts it (ex. npm's `dprint.cmd` on Windows, which runs through `cmd.exe`, or the `bin.cjs` Node wrapper an npm install without its install script has), because killing the wrapper doesn't kill the server, which then still holds its session (see `sessionId`). Resolve the wrapper to the executable it runs (ex. the `dprint(.exe)` in the `dprint` package), or otherwise kill the whole process tree (ex. `taskkill /T /F` on Windows, or the process group on macOS and Linux) and wait for the server itself to exit.
- An editor extension may also use the one a project installed with npm (ex. `node_modules/@dprint/<platform package>/dprint` found from the workspace folder and its ancestors, which includes the root of an npm workspace when a package of it is opened) so that the project's version of dprint is used. That executable is from the repository, so running it is the same as running code from the repository. Only do this in an editor that has workspace trust (ex. VS Code's Restricted Mode) once the user trusts the workspace.
- Only take the executable, its arguments, and its environment variables from the user's or the machine's settings, or from the workspace's settings once the user trusts the workspace. A project could otherwise point `DPRINT_CONFIG_DIR`, `XDG_CONFIG_HOME`, `HOME`, or `APPDATA` at a directory in the repository with a settings file that allows process plugins (`editor.allowProcessPlugins`) or a global configuration file whose process plugins run without asking, or point `DPRINT_CACHE_DIR` at plugins it compiled. A proxy in the environment (ex. `HTTPS_PROXY` or `NPM_CONFIG_HTTPS_PROXY`) is used without asking, so the project could also have every download sent to a server of its choosing. Keep in mind that the editor's environment may come from the project (ex. direnv, or opening the editor from its directory).
- The server keeps what the user decided for a session in a file in dprint's cache directory (see `sessionId`), like it keeps the `always` decisions in the approvals file next to its settings file, which is another reason to only take where those directories are from the user's settings or environment: a project that chose them could put a file there that allows what the user didn't. Make the session id in the editor extension (ex. a random UUID), and don't take it from the project (ex. the workspace's settings). It only selects the session's file in the cache directory.
- Show where a configuration file is when asking about it, and that it's outside the workspace when its `inWorkspace` is `false`, because a configuration file in an ancestor directory (ex. in `/tmp` or the root of a drive) may not be the project's.
- Show every string the server provides as plain text (ex. messages, labels, descriptions, titles, specifiers, uris, and configuration), because they may have text from a configuration file (ex. a plugin's URL or configuration). The server escapes the text from the project in an approval's `message` and its items' `specifier` and `registry` so it can't change how what's around it reads (see `ApprovalItem.specifier`). Text a client gets from a uri is from the project too (ex. the path of a `configFile`, `npmrcUri`, or `localUri`, which may have a newline or a bidi override in a directory name), so escape it the same way when showing it as text. So is the data of an approval, which the server doesn't escape because it's data: the `configuration` and `plugin` of its items and its `trustedConfiguration`, which is what the user approves a process plugin to run with (ex. a bidi override in a dprint-plugin-exec command would disguise it). Escape their strings the same way when showing them as text (ex. escape the JSON text, whose structural characters aren't affected), unless the editor shows those characters visibly (ex. a text editor that highlights invisible and bidi characters). An editor that renders Markdown or HTML in a notification may otherwise show a link from the project, which may run an editor command (ex. VS Code's `[label](command:...)` links in notifications), so escape it (ex. break up the `](` of a Markdown link).

### Compatibility

The editor extension says the versions of the `dprint/*` methods it supports and the server responds with the one it uses, so a server can support an old version alongside a new one:

- `versions` is a non-empty list of positive integers. A version the server can't read (ex. `"2"`) is ignored. A capability that isn't an object with `versions`, or that doesn't have a version the server supports, has the server leave `version` out of its capability (see "Starting the server").
- A version is added for a change that an editor extension needs to know about (ex. one that would break it). Fields may be added to results and to the params of the notifications and requests the server sends, and values to their string unions (ex. kinds and reasons), within a version, so ignore fields that aren't known and handle values that aren't known like the methods below say. What's added later is in new fields when it's more information about something (ex. a warning about a document that's formatted), and a new kind of result is only for something an older client can handle with its fallback.
- The server provides what the editor extension needs to handle a value it doesn't know where it can (ex. whether to keep claiming a document, when and how to show a message, and whether a decision allows what's approved), so prefer those fields over deciding from the value.
- A field or a value of params that changes what the server does (ex. a new `globalConfig` value or an `overrideConfig` key) is only added with a new version or a feature, and the client only sends it when the server responded with that version or feature, because the server rejects a value it doesn't know (`-32602`) and ignores a field it doesn't know. The decisions are the exception because the server offers them (a client only sends one of an approval's `decisions`). Likewise, a new kind of result that an older client would handle wrongly with its fallback is only returned when the client sent the feature for it. A field of a result, or of the params of a notification or request the server sends, that tells the client not to do something it otherwise does (ex. `onAnotherMachine`, or a field of `dprint/configChanged` like `schemaOnly`) is only added with a new version or a feature too, because an older client ignores it, and otherwise the server leaves out what the client must not use (ex. the uri). A new method or notification sent to the server is only added with a new version or a feature too, and only sent when the server responded with it, because an older server responds to a request it doesn't know with `-32601`, but ignores a notification it doesn't know without an error.
- A new notification or request from the server is only sent when the client sent the feature for it, which includes the language server protocol's (ex. `workspace/configuration`), because a client library may answer one by itself. The exceptions are the language server protocol's `window/showMessage`, `window/logMessage`, `$/progress`, and `textDocument/publishDiagnostics`, which the server may send in any version, so handle them. Ignore a notification that isn't known, and respond to a request that isn't known with `-32601`.
- The server ignores a setting it doesn't know or with a value of the wrong type, so a setting may be sent to an older server. The `initializationOptions` are sent before the version is known, so a new value of a setting that needs a version or a feature is only sent with `workspace/didChangeConfiguration` once the server responded with it.
- The glob syntax (see "Which documents dprint formats"), the error codes, and the positions being UTF-16 don't change within a version.
- The strings the server provides are plain text, not Markdown (see "Security").
- Params that are optional may be `null`, which is the same as not providing them. Results provide every field that isn't optional, which may be `null` where the types say so, and leave out an optional field instead of making it `null`.

### Features

Optional behavior is opted into with `features`, an object of the features the client supports (`{ [name: string]: true | object }`, where an object has options). The server responds with the ones it uses in its capability's `features` (`true`, or an object of the options it uses), which is left out when it doesn't use any, so a feature is only used when its name is in the server's `features`. `true` means the feature is used without any of the options the client sent (ex. a server that doesn't know an option of it), so only use an option when it's in the server's object. A feature that isn't known is ignored, and a feature applies to whichever version the server uses.

```json
{ "experimental": { "dprint": { "versions": [1], "features": { "fileUri": true, "watchFiles": true } } } }
```

- `fileUri` - A document is formatted as the file of its `fileUri` instead of its uri (see "Documents").
- `watchFiles` - The server registers the files it needs to be told about with `workspace/didChangeWatchedFiles` instead of the editor extension watching them (see "Lifecycle"). This is for an editor extension built on a language client library that already handles that registration (ex. one for Neovim, Emacs, or Sublime Text). It's only used when the client supports registering it dynamically with relative patterns (`dynamicRegistration` and `relativePatternSupport`), so watch the files yourself when it isn't in the server's `features`. The configuration files in the workspace folders are registered with a glob, and the configuration files that were used and the files they were resolved from (their `watchUris`) are registered by their canonical directory, which is never a symlink (ex. a configuration file in a `.config` directory that's a symlink is registered as what it points to), so the client reports their changes with canonical uris, which the server handles. It's registered again when they change. A configuration file on another machine isn't registered until reading it is allowed (the glob of the configuration files still covers the workspace folders). The ancestor directories of the workspace folders aren't registered either, so a configuration file that's created in one isn't reported by these registrations, and an editor extension with `watchFiles` still does one of the two things "Which documents dprint formats" says for that. On Windows and macOS, the glob matches the names in any case (ex. `Dprint.json`), like the server finds them.

### Lifecycle

- The server exits when the process of the `processId` of the `initialize` request exits, when the `exit` notification is received, or when its stdin is closed.
- Send `shutdown` and then `exit` to stop it, which stops the plugins and ends the session, so what the user decided for it is forgotten (see the restart command below). The server stops writing the file of the session on `shutdown` and deletes it on the `exit` after it (or when the client disconnects after it), so a language client library's stop, which sends both, ends the session, and the next server is started with a new `sessionId`. To stop a server and keep its session (ex. one that stopped responding), kill its process (tree), or send `exit` without `shutdown`, and start the next one with the same `sessionId` once it exited. For a request that hangs in a plugin, try `dprint/reset` first, which keeps the server and its session. The file of a server that's killed after `shutdown` is kept, and deleted a week later like the one of a server that crashed. Clear the diagnostics it published once it exits, because they aren't published again until a configuration file is used.
- Keep the server running for as long as the editor uses it. It reads a configuration file again for each request that uses it, so there's no need to start it again when one is created, changed, or deleted, or when the workspace folders change (send `workspace/didChangeWorkspaceFolders`).
- The server doesn't watch the workspace. Send `workspace/didChangeWatchedFiles` when a configuration file (`configFileGlobs`) is created, changed, or deleted, when the `configFile.uri` of a `dprint/configInfo` result changes that's outside the workspace folders (ex. a configuration file in an ancestor directory of a workspace folder) or is the one the server was started with (`kind: "cli"`, which sends `dprint/configChanged` without a `configFile` because it's used for every file), and when one of its `watchUris` changes (ex. a configuration file it extends). A watcher doesn't follow symlinks, because following one reaches what it points to, which may be on another machine that the user didn't allow reading (ex. a configuration file, a file it extends, or a `.config` directory that a `git pull` replaced with a symlink to a share). So watch a file by watching its directory rather than the file itself, with a watcher that doesn't follow the symlinks in the directory it watches, never watch a directory that's a symlink, or that's reached through one, apart from a workspace folder or one of its ancestor directories (the user opened a folder through it, and the server provides the editor's paths in it), and don't poll with a `stat` that follows symlinks (ex. VS Code's file watchers, or inotify of the directory, rather than a watch of the file or Node's `fs.watchFile`). Check whether a directory is a symlink each time a watcher of it starts, including when the directory was deleted and created again, because it may have been replaced with one since the server provided it. That's each directory of its path below the nearest workspace folder or ancestor directory of one, not only the directory itself (ex. `lstat` each one from there down), because `lstat` follows a symlink before the last component of a path (ex. `apps/web/shared/sub` after a `git pull` pointed `apps/web/shared` at a share). A watcher that starts once its directory was created (or created again) reports each watched file that's already in it as created, because the system doesn't report the files created with the directory before the watcher started (ex. a `git checkout` or `npm ci` that creates a directory with a configuration file in it). Watch a file whose directory doesn't exist yet by watching the nearest existing directory of its path that isn't a symlink (checked like any other, without following one) for the next name of the path, and then the next directory down as each one is created. The server checks again too that a directory the editor knows by another path (ex. one it opened a document through) still leads to where it did, once for each request that provides a path through it (a symlink that's pointed elsewhere isn't reported, so the server finds out for the next request that uses a path through it, or that a diagnostic was published through it, and then takes back the diagnostics published through it and sends `dprint/configChanged` for the configuration files the editor knew through it), and provides the canonical path otherwise. One that leads to the share of the directory it's an alias of is still an alias (ex. a workspace folder that's a symlink to a share). That's for every watcher: of the configuration files (including one whose `onAnotherMachine` is `true`), the files they extend, `.config` directories, and the `configFileGlobs` of the workspace folders. A file whose directory isn't watched then is noticed by the next request that uses it. `watchUris` leaves out a file whose directory may not be read, and the server sends `dprint/configChanged` once reading a configuration file on another machine is allowed, when a client may get its results again. The server doesn't follow a path that `workspace/didChangeWatchedFiles` reports to another machine either, and sends `dprint/configChanged` for a configuration file that's on another machine now with the editor's uri, so it's asked about. An editor extension that looks for configuration files itself (ex. in the ancestor directories of a workspace folder to know which ones to start the server for) doesn't follow a symlink to find out whether one is there (ex. `lstat` rather than `stat`, and not looking in a `.config` directory that's a symlink), stops looking at the root of a network share or a directory of an automounter like the server does (ex. it doesn't look up `/net/<host>/dprint.json` or `/net/.config`, which the automounter would look up as hosts), and doesn't follow a symlink to a directory when it searches the workspace (ex. `os.walk(followlinks=False)`, or the defaults of `fd` and ripgrep) where the editor's API allows that. When it doesn't (ex. VS Code's `findFiles`), say so in the editor extension's code, because the editor already follows those symlinks itself (ex. in its explorer and its search) and the user opened the workspace folder. The search of the ancestor directories, which the editor doesn't do, never follows a symlink. The server resolves a configuration file that it resolved before again to publish its diagnostics, and sends `dprint/configChanged` when what a configuration file formats may have changed. It doesn't resolve one that wasn't used yet because that would download and set up its plugins without the user doing anything. A change to a configuration file in `node_modules` is ignored because it's usually installed, so the `configFileGlobs` watcher can leave out `node_modules`, but still send the changes of `watchUris` there (ex. a shared configuration file a package provides that a configuration file extends). With the `watchFiles` feature, the server registers these files with the client instead, which are canonical directories that may be read (and the `configFileGlobs` of the workspace folders), so the library's watcher is used as it is, like the editor's search of the workspace.
- The `.gitignore` files also change which documents a configuration file formats. The server reads them again when they change, but doesn't watch them or send `dprint/configChanged` for them, so get the `dprint/fileInfo` of the open documents again when the editor sees one change, if it watches them.
- The server watches dprint's own files, which are outside the workspace and depend on the system: its settings file (ex. a domain added to `editor.trustedDomains`), its `editor-approvals.json` (ex. the user ran `dprint fmt` or allowed a process plugin in another editor window), and the global configuration file. When one changes, it resolves the configuration files that were used again and sends `dprint/configChanged` for the ones whose results changed.
- The server doesn't watch dprint's cache, so a plugin that was downloaded outside the editor (ex. by running `dprint config install` in a terminal) is only used once the configuration file is resolved again (ex. when the user approves the download, or with `dprint/reset`).
- When the server exits unexpectedly, start it again for the next request (with a delay that grows when it keeps exiting) with the same `sessionId` once its process exited, and get `dprint/configInfo` again for each configuration file the editor extension knows about and `dprint/fileInfo` for the open documents, because the new server doesn't know them (ex. the files they were resolved from) or publish their diagnostics until they're used. The server keeps what the user decided for the session in the file of its session id, which the new server uses, so it asks about the same approvals as the server that exited (ex. not about a download the user denied or a process plugin they allowed for the session, unless the approvals file changed since, which only means the user is asked again about the process plugins) without the editor extension sending any decision again. When the editor extension stops a server itself and keeps its session (ex. one that stopped responding), kill it or send `exit` without `shutdown`, and wait for its process to exit before starting the next one with the same `sessionId` (ex. kill it forcibly when it doesn't exit in time), because a server doesn't use the file of a session that a server that's still running uses (see `sessionId`). Get the results again like after any `dprint/approve` (see "Which documents dprint formats"), and get `dprint/pendingApprovals` for the status bar item once the results it depends on came back (without a `configFile`, it only has the configuration files the server used), or for each configuration file with its `configFile`, which resolves it.
- Keep which approvals the user was asked about (see "Approving" for what makes two the same), including after the server is started again, so a format on save doesn't ask about one again. That's the only thing about the decisions the editor extension keeps.
- A restart command is for forgetting what the user decided for the session, so stop the server with `shutdown` and `exit` and start a new one with a new `sessionId`, and forget which approvals were asked about too. The server deletes the file of a session when it exits after `shutdown`, and deletes the files that weren't written for a week when it starts (ex. of a server that crashed and wasn't started again), apart from the ones of servers that are running. `dprint/forgetDecisions` forgets the session's decisions of the configuration file (or of every one) apart from the downloads that were allowed, which stay allowed for the session (see `dprint/forgetDecisions`), and the file of the session forgets them too, so when it responds, forget which approvals were asked about of the configuration file it was sent with (with any `dirUri` when it was sent without one), or all of them when it was sent without a configuration file. Use `dprint/reset` instead when only the plugins need to start again (ex. after a plugin stopped responding or was changed).
- Every version of dprint that's installed uses the same directory for the files of the sessions (`editor-sessions` in dprint's cache directory), and each one deletes the files the others left there, so they keep to the same rules (the server's `SessionFile` has them with the reasons): a session's file is `<id>.json` and its lock is `<id>.lock` (with temporary files named `<id>.<8 lowercase hex digits>.tmp` while it's written); a file has the `version` of its format, which is raised for any change that an older server can't read or would read with another meaning (ex. a field that narrows an allow or adds denials), and a server ignores a file of a version it can't use (which only means the user is asked again), while a newer server may read the files of older versions, which it reads with the meaning they were written with (ex. what allowing a download covers is never broadened for the text of a download an older version kept); a file's process plugin allows and set-up grants are only used when its `approvalsFileFingerprint` matches the approvals file (a file without one fails closed), while its allowed downloads are used whatever it is, because the approvals file doesn't have download decisions; only the user may read and write a file on Unix (mode `0600`), because it may have the credentials of an `.npmrc` proxy setting; an item of a kind a server doesn't know is left out instead of the whole file, because the items are only shown; a denial of downloads is kept with what it denied rather than by its approval `id`, which a server makes again from it, and its `configKey` is only compared with the keys of the same version, so one that another version made another way still denies its downloads for any configuration file and is forgotten by the `configFile` it was shown with (`dprint/forgetDecisions` with what `dprint/decisions` provides for it); the text of a path download is compared by the paths it's of, because another version may escape a path another way (see `specifier`); a server holds an exclusive lock of the whole `<id>.lock` file while its session lasts (`flock` on Unix and `LockFileEx` on Windows) and neither reads nor writes `<id>.json` without it, and after taking the lock it checks that `<id>.lock` still names the file it locked, and otherwise takes it again, because another version may have deleted it while it opened it (ex. and another server created and locked a new one); it writes its file again when it was last written a day before by the wall clock (checking every hour, which counts the time the machine was asleep) and when the clock went back since; and apart from the files of its own session, which it deletes when it exits after `shutdown`, every version only deletes a file with one of those names whose modification time is more than a week from now (before or after) and whose session's lock it can take, deletes a `<id>.lock` only once its `<id>.json` is gone, doesn't create a lock to delete a file whose session has none, and doesn't look in the directory when it's a symlink. `dprint clear-cache` deletes the files of every session no server that's running uses, whatever their age, but older versions of it delete the whole directory, so a lock may be deleted under a running server. So don't keep other files in that directory or make it a symlink.
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

The server uses canonical paths, which have their symlinks resolved, but the uris it provides (ex. `configFile.uri`, `configFile.dirUri`, `watchUris`, the uris of diagnostics, and `dprint/configChanged`) are of the paths the editor knows the files by (ex. a workspace opened through a symlink, a Windows junction or `subst` drive, or `/tmp` on macOS being `/private/tmp`). The directories that the `watchFiles` feature registers are the exception, which are canonical (see `watchFiles`). It learns them from the workspace folders and the uris it's sent, so send the editor's own uris. A configuration file that's a symlink (or that's in a `.config` directory that's one) is known by where it was found or asked about, not by what it points to, and it's used for the directory it was found in. It's known by the names dprint looks up (ex. `.config/dprint.json`) when they're in another case on disk too (ex. `.Config` on a case insensitive file system). For `~/proj/dprint.json -> ~/dotfiles/dprint.json`, its `uri` is `~/proj/dprint.json`, its `dirUri` is `~/proj`, and its canonical path is in `watchUris`, so two symlinks to the same file are two configuration files, and every method that takes a `configFile` accepts the one of any result. A symlink to the global configuration file is such a configuration file too, not the global one. The CLI uses such a configuration file the same way when it finds it or it's given with `--config` (including `dprint lsp --config`, and a `~/` path or a `file:` url), but its walk of the subdirectories of where it runs skips a `dprint.json` or `.config` directory that's a symlink, so `dprint fmt` from an ancestor directory formats those files with the ancestor's configuration file, unless they're given as paths (or are in the directory a pattern starts in), while the editor and `dprint fmt` from that directory use it. `watchUris` are the editor's paths only through a workspace folder or one of its ancestors that the editor knows by another path, and otherwise canonical, because the editor doesn't watch through another symlink (see "Lifecycle"), so they have a configuration file's canonical path when it's reached through such a symlink. A configuration file that's a symlink whose target was deleted keeps its target in `watchUris`, so its being created again is reported. A directory the editor knows by another path is checked again for each request, and once it doesn't lead where it did, the server provides the canonical path through it, which is then another configuration file's identity (see "Lifecycle"). Another file in a directory the editor didn't tell the server about (ex. a file that a configuration file extends through a symlink) has its canonical path. The uris are encoded like the `url` crate encodes a file path (ex. `file:///C:/Users/user/project` on Windows), which may differ from the editor's (ex. `file:///c%3A/Users/user/project`), so compare them as file paths. On a case insensitive file system (Windows and macOS by default), a configuration file's `uri` (and the uri its diagnostics are published to) has the names dprint looks up, which may differ in case from the editor's uri of the file (ex. `.config/dprint.json` for `.Config/dprint.json`), so compare file paths case insensitively there, and match the `configFileGlobs` case insensitively too.

## Which documents dprint formats

An editor usually needs to know which documents an extension formats before formatting them (ex. to register as their formatter). Find the configuration files from the documents: the `configFile` of a document's `dprint/fileInfo` is the configuration file that's used for it, which may be in an ancestor directory of the workspace folder. An editor that needs to know them before a document is opened (ex. to register a formatter for their documents) may also search the workspace folders with the `configFileGlobs` and the ancestor directories of the workspace folders, leaving out what the editor excludes from searches (ex. build directories). Getting `dprint/configInfo` sets up a configuration file's plugins, so a configuration file found that way may download plugins that dprint wouldn't use for any document (ex. one in an ignored directory). Leave out `node_modules` too, whose configuration files are only used for the documents in it. A `.dprint.json` in a `.config` directory (other than the home directory's) isn't a configuration file dprint finds, so `dprint/configInfo` fails with `-32602` for it.

Get each configuration file's plugins with `dprint/configInfo`:

- `filePatterns` are globs of the documents its plugins may format, which are relative to the directory of the configuration file (`configFile.dirUri`). Register them as the documents the editor extension formats, and the cells of the notebooks they match, unless the status's `claim` is `false`.
- `otherFiles` are the other documents the configuration file formats, which a glob can't match:
  - `shebangs` - Files without an extension whose first line is one of the values, or starts with one followed by whitespace (ex. `#!/usr/bin/env node`). Register the files without an extension (ex. by checking the first line, or with every file) and get their `dprint/fileInfo`.
  - `associations` - The files of the patterns of a plugin's `associations` that can't be made relative to `configFile.dirUri` (ex. `../shared/**/*.ts`) or that have too many brace alternatives to provide a glob for each. The values are the patterns as they're written in the configuration file (relative to its directory), which are for showing the user and aren't globs to match, so treat every document like it may be formatted and get its `dprint/fileInfo`.
  - Treat a kind that isn't known like every document may be formatted, and get its `dprint/fileInfo`.
- The patterns are a superset of what's formatted, because the configuration file's `includes` and `excludes` aren't in them and a configuration file in a descendant directory may format the documents there instead. Before acting as a document's formatter (ex. when it's opened), get its `dprint/fileInfo` and only act as its formatter when its status is `ready` or its `claim` is `true`, so the editor's other formatters are used otherwise. The server says to keep claiming a document when the user needs to be told why it isn't formatted (ex. the configuration file has an error or something that isn't denied needs to be approved). An editor without a formatter registry (ex. one that runs `dprint/format` on save for every document) does the same: only format a document when the user saves it once it's claimed.
- When something needs to be approved, `plugins` and `filePatterns` are what they were the last time the plugins were set up for the configuration file, which is remembered across sessions (`plugins` is `null` when that isn't known for it, because what a plugin formats may depend on its configuration), so claim those documents and ask when the user formats one (see "Approving"). Only those documents are `approvalRequired`, and another document is `notMatched` or `noPlugin`.
- When `plugins` is `null`, they aren't known (ex. a plugin that needs to be downloaded) and `filePatterns` is empty. Every document in the directory is then `approvalRequired` with `claim` set to `false`. Don't claim the documents of the configuration file, so the editor's other formatters are still used, and show what needs to be approved instead.
- When the configuration file has an error, `plugins` and `filePatterns` are the ones it had before, so a typo doesn't change what the editor formats with it. Only the documents those plugins may format are `configError` with `claim` set to `true`. When they aren't known (ex. it had the error since the server started, so `plugins` is `null`), no document is claimed.

The globs use `/` to separate path segments and are relative without a leading `/` or `./`. A `**` segment matches any number of segments (including none), `*` matches any characters within a segment and `?` one character, which includes names that start with a period (ex. `**/*.json` matches `.config/.eslintrc.json`, like picomatch's `dot: true`), and `[...]` matches one of the characters of the class, which may have ranges (ex. `[a-z]`) and be negated with a leading `!` (ex. `[!.]`), and doesn't match `/`. There aren't braces or negated patterns. A character that's otherwise special (`*`, `?`, `[`, `{`, or `}`) is in a class of its own when it's matched literally (ex. `[*]`), and file extensions and names are matched case insensitively with a class of both cases (ex. `**/*.[tT][sS]`). These are VS Code's globs. Check the options of another glob library (ex. picomatch and minimatch skip names that start with a period by default, and Python's `fnmatch` has `*` match `/`).

The results stay valid until the server says otherwise, so get them again only when:

- The server sends `dprint/configChanged` for the configuration file, or without a `configFile`. It sends it whenever what a configuration file formats may have changed: when one the editor said was created, changed, or deleted, when a file it was resolved from changed, when resolving it for a request has a different result (ex. a remote configuration file it extends changed), when what needed to be approved was approved or denied (including outside the editor, which the server notices in dprint's own files), when a configuration file is on another machine now, for the configuration files the editor knew through a directory that doesn't lead where it did (see "Lifecycle"), and without a `configFile` when `useGlobalConfig`, the workspace folders, or whether there's a global configuration file changed.
- `dprint/approve`, `dprint/forgetDecisions`, or `dprint/reset` responded. A `dprint/configChanged` may also be sent for the same change, so getting the results again once for both is enough (ex. by waiting a moment).
- The server was started again.

When it's sent, get `dprint/configInfo` for the configuration file and `dprint/fileInfo` for the open documents it may be used for, because a change to its `includes` or `excludes` changes whether they're formatted. Those are the documents in its `dirUri`, the documents its `associations` may match (see `otherFiles`), every document for the one the server was started with (`kind: "cli"`), and the documents without a configuration file when it was created. A configuration file that's created in an ancestor directory outside the workspace folders isn't reported by those watchers, so either watch the `configFileNames` in the ancestor directories of the workspace folders like the rest (non-recursively, and stopping where the server stops looking), along with the `dotConfigFileNames` of each one's `.config` directory when that isn't a symlink (checked like any other watched directory, including when it's created), or get `dprint/fileInfo` again for a document without a configuration file when it's focused or saved. Without a `configFile`, get them for every configuration file and open document. The results of `dprint/format` say whether to claim a document (`claim`, including a `failed` result's) and `dprint/approve` says whether its configuration file is used (`status`), which replace what was known before for them.

A configuration file is identified by its `uri` and the directory it's used for (`dirUri`), which the server always provides together. The global configuration file has results for each directory it's used for (ex. each drive). Send the `dirUri` with the `uri` when a method takes a configuration file and it's known (ex. from a result), which is needed for the global configuration file and the one the server was started with, and isn't for one the editor found (a configuration file in the workspace is used for the directory it's found for, its `dirUri`). Keep what's known about one by both, and treat a `dprint/configChanged` whose `configFile` doesn't have a `dirUri` like it's for each one.

## Approving

Opening a project isn't the same as choosing to run what its configuration file says, so the server only uses a configuration file once the user approves what it needs:

- **Downloads** - A download sends a request to the domain it's from, which a configuration file could use to send information to a server of its choosing (ex. that the project was opened and from where). To limit that, the server only downloads plugins and the configuration files that a configuration file extends from these trusted domains (and their subdomains) without the user approving it: `registry.npmjs.org`, `plugins.dprint.dev`, `github.com`, `githubusercontent.com`, and the ones the user added to the `editor.trustedDomains` setting in dprint's settings file (none of them when the settings file isn't used, see "Process plugins" below). This includes each redirect, and reading a file on another machine (ex. a UNC path on Windows, a path in `/net` or `/System/Volumes/Data/net` on macOS and Linux, or one in a directory of an automounter like `autofs` or where a share is mounted when it's looked up (ex. the directory of a direct map of `autofs`, or an AFS cell on Linux), which the system connects to when it's read, including through a symlink) is approved like a download, unless it's on the same share as the configuration file, where the symlinks on the share are followed too (ex. one that points at another machine is approved). The server follows a path like the system does, so on Windows the `..` of a path and of a symlink's target is applied to its text first (ex. `link\..\x` is `x` even when `link` is a symlink), and an alternate data stream is of what its file resolves to (ex. `link:stream` when `link` is a symlink). On Windows, a path is also approved like one on another machine when it has a name that Win32 changes (one ending with a dot or a space) or opens a device for (ex. `LPT1`, which may be a printer on another machine), or when the file of a stream in it can't be looked up, or when a `..` would leave the root of a verbatim path (ex. `\\?\C:\..\x`), which the system doesn't apply, because the server can't tell where it is. Only a path on a drive or a volume in a verbatim path (`\\?\Volume{...}\`) is local there, not a device path (ex. `\\.\Volume{...}\` or `\\.\C:\`), whose root is `\\.\`, so its `..` may remove the device (ex. `\\.\Volume{...}\..\UNC\server\share` is a UNC path). That's each file a configuration file names (ex. in `plugins` or `extends`) and each npm package in `node_modules` that it uses, along with the files in the package. A path that reaches another machine through a symlink is shown with the path on that machine (ex. `C:\project\tools\plugin.wasm -> \\server\share\plugin.wasm`, where an `->` in either path is shown as `-\u{3e}`), and allowing it allows that, so the user is asked again when the symlink points elsewhere. Allowing a directory (ex. an npm package) also allows what's in it, apart from a file or directory below it that's denied for any configuration file (and what's in that directory), so the most specific decision is used, which is an allow when the same path is allowed exactly. Once reading a path on another share is allowed, the server follows the symlinks on that share too, like on the configuration file's share, so one that points at a third machine is approved on its own (ex. `/project/node_modules/pkg/plugin.wasm -> /net/other/x/plugin.wasm` after allowing `/project/node_modules/pkg -> /net/server/share/pkg`). The other files that the server reads on its own are approved like a download when they're on another machine too:

  - The configuration files it looks for: the ones in a document's directory and each ancestor directory (`dprint.json` and the other names, and a `.config` directory), the ones in the ancestor directories of a configuration file with `"inherit": true`, and the ones in an npm package that a configuration file extends. A configuration file that's found on another machine is the `configFile` of its approval, with `onAnotherMachine` until it's allowed, and one in an ancestor directory isn't used instead, which the CLI wouldn't do. A configuration file the editor sends (ex. to `dprint/configInfo` or `dprint/pendingApprovals`) is checked the same way before it's read, so its approval is the result until it's allowed, and `dprint/forgetDecisions` forgets the decisions for it without reading it (the ones for the path it's at, not the one on the other machine).
  - The `.npmrc` files (other than the user's own `~/.npmrc`), before an npm package that what they may set applies to is downloaded or found in dprint's cache, because they may set where it's from (ex. a company's registry for a scope), so it isn't resolved differently than the CLI would.
  - The `.gitignore` files, and `.git` and `.git/info/exclude`, which are ignored when they're on another machine instead (a directory whose `.git` is then isn't the root of a repository).

  What's above the root of a share isn't looked in (ex. `/net/<host>` on macOS, where `/net/.npmrc` would be a host named `.npmrc`, or `/home/<user>` when an automounter mounts the directories in `/home`). Reading a path on another machine is its own approval of the `downloads` kind, whose items are of the `networkPath` kind (with the path as the `specifier`) and whose `message` doesn't suggest `editor.trustedDomains`, which can't trust a path. A download is also only sent through a proxy that an `.npmrc` file sets for npm packages (`https-proxy` or `proxy`) once the user approves it, unless the file is the user's own `~/.npmrc`. That includes the files in the configuration file's directory and every ancestor directory, because they may be the project's, and the project would otherwise choose a server that receives every download along with the credentials in the setting, which may have the values of the user's environment variables (ex. `https://${GITHUB_TOKEN}@proxy.example`). Until then, a download that would go through it needs approval instead of going another way. A setting of such a file that isn't the url of a proxy (a scheme, a host, and optionally a port and credentials) is ignored with a warning, so it isn't asked about (ex. one with a path or with text after the url). Each proxy is its own approval of the `downloads` kind, so its message and decision are only about it and not hidden among the urls (ex. when a long message is cut off). What's in dprint's cache doesn't need to be downloaded (ex. from running `dprint fmt` or `dprint config install`), and neither does a plugin that's a file (ex. in `node_modules`). Allowing a download allows each of its exact urls for the rest of the session, including for the other configuration files that need them, which are then used too. Only the url of an npm registry also allows every url under it (ex. each package and its tarball), which its item's `registry` says and its `message` says too, and any other url only allows itself (ex. `https://example.com/` doesn't allow `https://example.com/other.json`). Denying a download also denies what the same configuration file needs of it later (ex. only some of the urls once another configuration file allowed the others), and an npm registry or a directory that's allowed doesn't allow a url, file, or directory under it that's denied for any configuration file (or what's in that directory), which only allowing it (or something in it) exactly does. A denial of downloads that were each allowed exactly since (ex. for another configuration file) isn't kept, because it wouldn't do anything, and the result's `notice` says that they're still allowed for the session and how to be asked again. Allowing a proxy allows the text of its setting in that `.npmrc` file, so the user is asked again when the setting changes. A decision that isn't `always` is only for the session, so another server (ex. of another editor window or another `dprint` executable) asks again.
- **Process plugins** - They aren't sandboxed, so running them needs to be approved for the configuration file with each plugin's configuration, unless the user set `editor.allowProcessPlugins` to `true` in dprint's settings file (see [the language server docs](../website/src/lsp.md)). The settings in `editor` aren't used when the settings file can't be read or has a property or a value in `editor` that the server doesn't know (ex. a setting of a newer version of dprint, or an entry of `editor.trustedDomains` that isn't a domain). The server then fails closed to the narrowest state, so that a setting of a newer version that narrows what's allowed (ex. not trusting a default domain, or never allowing process plugins) is honored by older versions: it trusts no domains, not even the default ones, so every download needs approval, and it doesn't use the stored `alwaysAllow` decisions for process plugins, but still applies the stored denials, like with an approvals file it can't use (see below). An `alwaysAllow` the user decides then is still stored for the versions that use the settings, and is used for the session. The `message` of an approval then says why, instead of suggesting to change the settings. Running dprint with the configuration file (`dprint fmt`, `dprint check`, or `dprint config install`) approves them because the user ran it. Approving a plugin's configuration doesn't approve the files of the project that it runs (ex. a script in dprint-plugin-exec's `commands`), so a change to one of them (ex. after a `git pull`) runs without asking.

Nothing is formatted with the configuration file until then, not even by its Wasm plugins, because formatting only some files or formatting them with other plugins would format differently than the CLI. The results of `dprint/format`, `dprint/fileInfo`, and `dprint/configInfo` then have the `approvalRequired` reason and what needs to be approved (see "Which documents dprint formats" for which documents). The `approvals` of any result are what to ask about, whatever its reason, but only ask when the user wants to format with the configuration file (see below):

```ts
interface PendingApproval {
  /**
   * Identifies exactly what's approved, which changes when what's shown does (ex. the configuration of a process plugin, or once its
   * process plugins are set up) and is the same in another server of the same version of dprint, but may change between versions, so
   * recognize the same approval by what it approves instead (see "Approving"). Only send a decision for the id that was shown to the
   * user.
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
   * The message to show the user when asking, which says what's approved and why. It names everything that's approved,
   * including where each item is from (ex. the registry of an npm plugin or the `.npmrc` file of a proxy), apart from what the
   * process plugins run with (`configuration` and `trustedConfiguration`). A client that can't show the items shows it, and an
   * item property that isn't known doesn't need to be shown. The text it has from the project (ex. a url, a path, or the
   * setting of an `.npmrc` file) is in backticks (ex. `` `https://example.com/plugin.wasm` ``), so it's clear where it starts and
   * ends when the message is shown as plain text. It's escaped like an item's `specifier`, and a backtick and a character that
   * looks like one are escaped too (ex. `\u{60}`), so the text can't end the quotes or change how the rest of the message reads.
   */
  message: string;
  /**
   * The trusted configuration of the configuration file, including what it extends, which the process plugins that weren't set up
   * before run with once they're allowed (what each one runs with isn't known until it's set up).
   */
  trustedConfiguration?: object;
}

interface ApprovalItem {
  /**
   * The url, path, or npm specifier (ex. of a plugin with its checksum). A path that reaches another machine through a symlink is followed
   * by ` -> ` and the path on that machine (see "Approving"), and an `->` in a path is shown as `-\u{3e}` so the one between them
   * is the only one. For an `npmrcProxy`, it's the text of the setting before its environment variables are replaced (ex.
   * `https://${GITHUB_TOKEN}@proxy.example`), which shows what would be sent to the proxy.
   *
   * It's from the project, so it's escaped to be shown as it is without changing how what's around it reads:
   *
   * - These characters are shown as `\u{` followed by the code point in lowercase hex and `}` (ex. `\u{a}` for a newline or
   *   `\u{202e}`), by their Unicode properties:
   *   - A control character (general category `Cc`, ex. a newline), a separator other than a plain space (`Zs`, `Zl` or `Zp`,
   *     ex. U+00A0 or U+2028), an invisible format character (`Cf`, ex. a bidi override like U+202E, which shows the text after
   *     it backwards, or a zero width space), any other default ignorable code point (`Default_Ignorable_Code_Point`, ex. a
   *     variation selector or a Hangul filler), and a code point that's unassigned in the version of Unicode the server uses
   *     (`Cn`, which includes the noncharacters), so a character that a later version of Unicode adds (ex. a format character)
   *     is escaped by it.
   *   - A combining mark (`Mn` or `Me`) at the start, which would change what's shown before it.
   *
   *   So the same characters can be escaped in JavaScript with `/[\p{Cc}\p{Cf}\p{Cn}\p{Z}\p{Default_Ignorable_Code_Point}]/u`
   *   for a character other than a space, and `/^[\p{Mn}\p{Me}]/u` for the first one. The server currently uses Unicode 16, which
   *   a later version of dprint may raise, so a character that a later version of Unicode added is escaped by the server (it's
   *   unassigned there) and may not be by a client with newer tables, and a client with older tables escapes more. The server compares the text
   *   of a path download that another version wrote (ex. in a session file) by the paths it's of, and a path that isn't valid
   *   Unicode can't be allowed, because its text has a replacement character for what isn't, so it can't be told apart from
   *   other paths: its approval only offers denying it, and its `message` says why.
   * - That's the only kind of escape. A backslash that's followed by `u{` is shown as `\u{5c}` so the text can't look like an
   *   escape, and any other backslash is shown as it is, so a Windows path stays readable (ex. `C:\new` is the text `C:\new`).
   *
   * So the text can be read back exactly by replacing each `\u{...}` with its character, though there's no need to.
   *
   * Each path of a `networkPath` is escaped like the text the `message` quotes, because the message quotes the specifier as it is: a
   * backtick and a character that looks like one are escaped too, and a combining mark at the start of each path (after ` -> `
   * too). A client that shows the path of the `localUri` next to it escapes it like the rest of the text from the project, so a backtick
   * in it reads differently than in the specifier (ex. `C:\proj\a\u{60}b\x -> \\srv\s\x` next to `` C:\proj\a`b\x ``).
   */
  specifier: string;
  /**
   * A config file is one that a config file extends, an npmrcProxy is a proxy that an `.npmrc` file sets, which downloads are sent
   * through once it's allowed, and a networkPath is a file or directory on another machine that's read (ex. a plugin, a
   * configuration file, an npm package, or an `.npmrc` file). Show a kind that isn't known by its specifier. An npm registry's item
   * is `plugin` when what was downloaded from it isn't known (ex. a decision kept from another version of dprint).
   */
  kind: "plugin" | "config" | "npmrcProxy" | "networkPath" | (string & {});
  /**
   * For the download of an npm registry itself (any registry, including the default one), it's the registry, which is the specifier
   * too, and allowing it allows every url under it unlike another url, while the item's `kind` is what's downloaded from it (`plugin`
   * or `config`). Show that allowing it allows every package and url under it. Otherwise, it's the npm registry an npm plugin is from
   * when it isn't the default one (`https://registry.npmjs.org/`), which an `.npmrc` file can set (ex. the project's for a scope).
   * Show it with the specifier then because the specifier doesn't say where the plugin is from. It's escaped like the specifier.
   */
  registry?: string;
  /** The uri of the `.npmrc` file of an `npmrcProxy` as the editor knows it, which is shown with it (ex. with a way to open it). */
  npmrcUri?: string;
  /**
   * The uri of the path in the project of a `networkPath` that reaches the other machine through a symlink (the path before ` -> `)
   * as the editor knows it, which may be shown with it (ex. relative to the workspace folder). It's the local side of the item, not
   * where it's read from. Don't open it before it's allowed, because that follows the symlink and reads it from the other machine.
   */
  localUri?: string;
  /** The plugin when it was set up before. */
  plugin?: FilePlugin;
  /** The configuration a process plugin that was set up before runs with (ex. the commands of dprint-plugin-exec). */
  configuration?: unknown;
}

interface ApprovalDecisionOption {
  /** The decision, which is what `dprint/approve` is sent with. */
  decision: ApprovalDecision;
  /** What to show the user. */
  label: string;
  /** Whether it allows what's approved, which is otherwise denied. */
  allows: boolean;
  /** Whether it's remembered after the session ends (in the approvals file), which is otherwise only for the session. */
  always: boolean;
  /** What it does (ex. for how long and for what), which is shown with it (ex. as a tooltip, or in the message when the editor can't show one). */
  description: string;
}

/** Show a decision that isn't known by its option's `label` and `description`. */
type ApprovalDecision = "allow" | "alwaysAllow" | "deny" | "alwaysDeny" | (string & {});
```

Ask about the approvals of a result when the user wants to format with the configuration file (ex. a format on save or a command), and not when a project is opened, because the editor makes requests without the user doing anything. A format the user didn't run as a command only asks when the result's `claim` is `true`, or when the user chose the editor extension as the formatter of the document's language (ex. VS Code's `editor.defaultFormatter`). Show the approval's `message` instead of the `message` of the `approvalRequired` result, ask the same about a kind that isn't known, and only offer the approval's `decisions`. Show the configuration file (and that it's outside the workspace when its `inWorkspace` is `false`) and what will be downloaded or run so the user knows what they're approving, apart from the `message` because it may be cut off when it's long: each item's `specifier` (with the `npmrcUri` of an `npmrcProxy` and the `localUri` of a `networkPath`), each item's `registry` when it has one, each item's `configuration`, and the `trustedConfiguration` when there is one, which is what a process plugin that wasn't set up before runs with. It includes what the configuration file extends, so opening the configuration file isn't enough to see it. It may be large, so it can be collapsed (ex. behind a "Show Configuration" button) as long as the user can see it before deciding. Before it's allowed, don't open or read what's on another machine, which is the `localUri` of a `networkPath` item and a `configFile` whose `onAnotherMachine` is `true` (ex. don't offer to open the configuration file then), because that reads it from the other machine. Also show that the configuration file needs approval somewhere that doesn't interrupt the user (ex. a status bar item that asks when it's clicked), because a denied approval isn't asked about again and the editor doesn't act as the formatter of its documents. `dprint/pendingApprovals` has what needs to be approved for that: get it again when the results are gotten again, once they came back (see "Which documents dprint formats"), and close a prompt whose approval isn't pending anymore (ex. it was decided in another window). Don't count such an approval as asked, and get the results of its configuration file again (or `dprint/pendingApprovals` with its `configFile`), formatting the documents that waited on it again when its `status` is `ready`.

The editor formats without the user doing anything in between (ex. a "save all" of many documents), so don't let asking turn into a storm of prompts:

- Show one prompt for an approval `id` at a time, which every format waiting on it shares, including the formats of other servers the editor extension runs (ex. a global configuration file that's used in two projects), and send the decision to each server that has a format waiting on it, which is the exact same approval, because what the user decided for the session isn't shared between servers (see "Starting the server"). The asked-once marks are for each server.
- A format the user didn't run as a command asks about the same approval at most once until the user decides about it, even when the prompt is closed without deciding, because that leaves it pending, and also after the server is started again. The same approval is the same configuration file (its `uri` and `dirUri`), `kind`, and set of its items' `specifier`s (in any order), rather than the same `id`, because its `id` changes when what's shown about it does (ex. once its process plugins are set up). Once the user picks a decision in a prompt, stop counting the approval as asked, whatever `dprint/approve` returns, so it's asked about again when it's pending again later (ex. the configuration of a process plugin changed after the user allowed it, a server that was started again didn't keep the session, the decision was forgotten in another window, or the settings file stopped the stored allows from being used). Only a prompt that's closed without a decision keeps the approval asked. So after a request of `dprint/approve` fails, what's pending afterwards is handled like below (see `-32803`). A command or the status bar item asks again, including about what was denied.
- Don't make a save wait on the prompt. Once it's approved, format the document's current text again, which may have changed in the meantime, and only apply the edits when the document didn't change while it was formatted. Save it again when it was saved before and is unchanged since.

Several may need to be approved at once (ex. downloading a plugin and running a process plugin). Ask about them one at a time, or show them together and send the decisions one at a time, sending the next only while its `id` is still in the previous result. Send what the user decided with `dprint/approve` and take what to ask about next from its result, which is what needs to be approved now (ex. a configuration file that a downloaded one extends or the process plugins of a downloaded configuration file). Stop when the result has nothing left to ask about, which is nothing that isn't denied for a format the user didn't run as a command, and nothing at all for a command or the status bar item, and format the document again when its `status` is `ready` after a decision that allows (not because of a denial). After a denial that succeeded, don't ask about anything else in that request, because the configuration file isn't used, and a later format or command asks about what's left like about any approval. When the approvals were shown together, still send the decisions the user made for the others while their ids are in the result, because only asking stops. When the `status` after a decision that allows isn't `ready` otherwise (ex. allowing a download revealed an error in the configuration file), show its `message` like a result of a command the user ran, because the user is waiting on the decision they made.

`-32803` means what was approved changed before the decision was received (ex. the configuration file changed), so the decision wasn't used, whether or not its `data` has `current`. Its `data` has what needs to be approved now for the configuration file and whether it's used (`current`) when they're known. When the request is cancelled or fails otherwise, the decision may or may not have been used (ex. the server exited before it received it and was started again). After any failure of `dprint/approve`, `-32803` or another one, handle it the same way, sending the decision at most twice for the same `id`:

1. Get what's pending: its `current` when it's `-32803` with one, and otherwise `dprint/pendingApprovals` for the approval's configuration file (with its `configFile`). Handle that result like a result of `dprint/approve` for the status and the results to get again (see "Which documents dprint formats"), and decide what to send or ask next as below.
2. When the approval with the same `id` is still pending and isn't `denied`, send the same decision again once, because it's exactly what the user saw. Don't send an allowing decision again for one that's `denied`, because that would replace a denial made since (ex. an `alwaysDeny` in another window).
3. Otherwise, for a decision whose option's `allows` is `false`, don't ask again right away: send the same denial once for the approval of the same `kind` and `configFile` that replaced it and isn't `denied`, if there is one, it has that decision, and its item `specifier`s are all among the denied approval's item `specifier`s (so a plugin added since isn't denied without the user seeing it), and don't ask about anything else in that request, so a later format asks about what's left like about any approval. Never send an allowing decision that the user didn't make for what's shown.
4. Otherwise, for a decision that allows, ask about what's next like after a result of `dprint/approve` (ex. the approval that replaced the one that was allowed).
5. Show the original error only when the decision is still pending afterwards, which is when the approval with the same `id` is still pending and isn't `denied` because it wasn't sent again or sending it again failed too, or when getting what's pending failed. When an allowing decision's approval is `denied` afterwards (ex. an `alwaysDeny` in another window), tell the user it was denied since instead.

The decisions that are `always` are stored in an `editor-approvals.json` file next to dprint's settings file (`files.approvalsUri`), which the server notices when it changes (see "Lifecycle"). Every version of dprint that's installed uses the same file (ex. a project's npm dprint and a global one), so a decision stored by one is used by the others that can read it (including an `alwaysAllow` of process plugins that another one doesn't have set up, ex. after an upgrade or with another cache directory, which sets them up when the trusted configuration has the configuration that was allowed, and asks once they're set up when it isn't theirs), and an older version keeps what a newer one added to it that it doesn't know (ex. a property of a decision or another way of identifying a plugin's configuration). A newer version only stops older ones from using the file for a change they can't safely ignore, which can't be undone (ex. a change to how denials are read). A change that narrows what an allow covers is made to each allow instead (ex. with a new version of its `configHash`, which an older version doesn't use), so older versions still use the file. Every version applies a denial (which is stored for any configuration of the plugin) over an allow of the same configuration file and plugin in a format the version that stored the denial may not know (ex. a newer `configHash` version), whatever their times say, because that version couldn't time the denial after it or remove it. A version that stores an allow removes the denials of it. That can't narrow what an older version does without asking (ex. downloading from a trusted domain), because one that refuses the file still uses the settings file's trusted domains, so a later decision like that is an `editor` setting in the settings file instead, which an older version that doesn't know it fails closed for (see "Process plugins"). Every version keeps the file strict JSON (without comments or trailing commas), because one that can't parse it loses the denials in it, and every version that writes to it takes the same lock for the whole read and write, an exclusive lock of the whole `.editor-approvals.lock` file in its directory (`flock` on Unix and `LockFileEx` on Windows), and never writes to it without the lock, so they don't lose each other's decisions. No version deletes or replaces `.editor-approvals.lock`, because a version isn't required to check that the path still names the file it locked. One that can't take it (ex. another process holds it for too long, or the file system doesn't support locks) doesn't store the decision, which is then only used for the session (the result's `notice` says it couldn't be stored). So don't edit it from an editor extension, and use `dprint/forgetDecisions` instead. How a decision's configuration file and plugin are written in it is part of its format, which every version keeps, because a decision only replaces one with the same texts: a local configuration file is the canonical path of the directory it's used for joined with the names dprint looked up (ex. `.config/dprint.json`, or the file's own name when it was given with `--config`), and the global configuration file is the canonical path of the directory it's in joined with its name, which is where it was found when that isn't its canonical path (ex. a symlink, or those names in another case on disk), and on Windows the canonical path keeps `\\?\` when it can't be left out (ex. a path that's too long, or `\\?\UNC\` for a share); a remote one is its normalized url, and an npm one is its specifier; and a plugin is its url or specifier with `@` and its checksum when it has one. A version that changes any of them also stores a denial under each older form it can make and removes the allows under them. Every version forgets a decision by the `configFile` that `dprint/decisions` lists for it, even when that isn't how this version would write it (ex. the project was moved since). An older version that can't use the file then still applies the denials in it, but ignores what it allows and never writes to it: an `always` decision is only used for the session (the result's `notice` says to update dprint to remember it), and `dprint/forgetDecisions` only forgets the decisions of the session (its `notice` says to update dprint to forget the stored ones). The most recent decision is used, but an `alwaysDeny` that's stored after a server received a decision that allows for the session replaces that decision whatever their times say (ex. one from another machine whose clock is behind, or one made after forgetting a decision whose time was ahead). `dprint/decisions` lists what the user decided (ex. for a command that shows them), and `dprint/forgetDecisions` forgets what the user decided for a configuration file, or for every configuration file, so they're asked again (ex. a command to undo allowing a process plugin).

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
  }
  /** A kind that isn't known (see below). */
  | {
    kind: string & {};
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

Show the message of a result, and of a `notice`, like its `show` and `severity` say, with its `actions`. A format that the editor can't tell apart from one the user didn't run as a command (ex. a formatter that's also run on save) isn't a command. For `oncePerConfigFile`, the configuration file is the result's `configFile` (its `uri` and `dirUri`), or the message alone when it's `null` or the result doesn't have one (ex. a `notice` of `dprint/forgetDecisions`). The server doesn't show them to an editor extension's user. Treat a kind that isn't known like `notFormatted` with the fields it has, which is the same as for a `FormatStatus`: its `approvals` are asked about, it doesn't claim the document without `claim`, and its `message` is only shown when the user ran a command without `show` (ex. a generic message when it doesn't have one).

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
type FormatStatus = { kind: "ready" } | ({ kind: "notFormatted" } & NotFormatted) | { kind: string & {} };

interface FilePlugin {
  name: string;
  version: string;
  configKey: string;
}

interface ConfigFileInfo {
  /** A file, https, or npm uri. */
  uri: string;
  /**
   * The directory whose files it formats, which is the directory it's in unless that's a `.config` directory (other than the
   * home directory's), even when it's a symlink to a file in another directory, and the directory it's used for when it's the
   * global configuration file (ex. the root of a drive). It's the directory its patterns are relative to for a remote or npm
   * configuration file.
   */
  dirUri: string;
  /**
   * How it was chosen: found in an ancestor directory, the global configuration file, or the one the server was started
   * with (`dprint lsp --config <path>`). Treat a kind that isn't known like "local".
   */
  kind: "local" | "global" | "cli" | (string & {});
  /** Whether it's in a workspace folder, which the global configuration file and one in an ancestor directory of the workspace folders aren't. */
  inWorkspace: boolean;
  /**
   * Whether it was found on another machine and reading it isn't allowed yet (see "Approving"), which is left out otherwise. Don't
   * open or read it then (ex. with a button in the prompt or a command that opens the configuration file of a document), because
   * that reads it from the other machine, which the approval is for. Its directory is watched like any other, which is never when
   * it's a symlink that isn't a workspace folder or one of its ancestors (see "Lifecycle"). The server sends `dprint/configChanged`
   * once it's allowed, and when a configuration file becomes one on another machine (ex. a `git pull` replaced it with a symlink
   * to a share). So don't open a configuration file from an older result or action (ex. a notification's "Open Configuration
   * File") when the latest result the editor extension has for it has `onAnotherMachine`, which is the latest `configFile` with
   * its `uri` (and `dirUri`) of a `dprint/configInfo` or `dprint/fileInfo` result, because both have the server's view of the
   * configuration file, and otherwise open it as usual. A `dprint/configChanged` that names it (or no configuration file) and
   * a restart of the server make it unknown again. The server takes back the diagnostics it published for it then by
   * publishing empty ones, and the ones it published through a directory the editor knew by another path once that doesn't lead
   * where it did (when it also sends `dprint/configChanged` for the configuration files the editor knew through it, whose
   * diagnostics are published again once they're resolved again), so don't drop them for this. This is a best effort (see
   * "Security").
   */
  onAnotherMachine?: boolean;
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
   * file when not provided, and the directory to find the configuration file the server was started with from (`kind: "cli"`),
   * which is the server's current directory when not provided. For `dprint/decisions` and `dprint/forgetDecisions`, a
   * configuration file without one is of each directory it's used for instead.
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
  /**
   * The local files that it was resolved from (ex. a configuration file it extends), and the canonical path of a local
   * configuration file when the `configFileGlobs` watcher doesn't report a change there, which is when it's a symlink (or is in
   * a `.config` directory that's one, ex. a `dprint.json` that's a symlink to a `base.json` or to a file in `node_modules`), when
   * its names are in another case on disk, and when it's reached through a directory that's a symlink the editor doesn't watch
   * through. The editor watches them too, by their directories (see "Lifecycle" for when one that's a symlink isn't watched).
   * Each is the editor's path when that's through a workspace folder or one of its ancestors that it knows by another path, and
   * the canonical path otherwise, which the editor may watch. A file whose directory may not be read (ex. it's on another machine now) is left out.
   */
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
  /** The `id` of the approval. */
  id: string;
  /** The `decision` of one of the approval's `decisions`. */
  decision: ApprovalDecision;
  workDoneToken?: string | number;
}

/** The configuration file the approval was for and what needs to be approved for it now. */
interface ApproveResult extends ConfigApprovals {
  /**
   * What to tell the user about the decision, or about one made before for the same configuration file (ex. a denial wasn't kept
   * because what it denied was each allowed exactly for another configuration file since, so it's still allowed for the session,
   * or an `always` decision couldn't be stored, or the approvals file is from a newer version of dprint that this one can't write
   * to, so it's only used for the session). The server keeps such a decision for the session like one that isn't `always` (see
   * "Approving"). It also says when the file of the session couldn't be written or deleted, so what the user decided for the
   * session before may come back if the server is started again, and when the session's decisions aren't kept yet because another
   * server that's running uses the session id or its lock can't be taken (see `sessionId`).
   */
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
   * what was denied when the decision was to deny it and the denial was kept (a denial of downloads that were each allowed
   * exactly since isn't, which the `notice` says), and otherwise it may have something else that needs to be approved.
   */
  approvals: PendingApproval[];
  /**
   * Whether the configuration file is used now, which isn't "ready" when it has an error or was deleted, even when nothing needs to
   * be approved. It's for the configuration file, so get the `dprint/fileInfo` of a document to know whether it's formatted.
   */
  status: FormatStatus;
}
```

The decision is used once the server receives it, even when the request is cancelled. The approval is for its `configFile`, even when a document it was asked about for uses another configuration file now. Allowing it downloads and sets up what was allowed, and then the configuration file is resolved again for the result. It fails with `-32803` when nothing is pending with the id. A denial of downloads that were each allowed exactly since isn't kept, because it wouldn't do anything, and the result's `notice` says so.

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

Forgets what the user decided about the approvals of a configuration file, or of every configuration file, including the decisions that are `always` and the ones from running dprint (ex. `dprint fmt`), so the user is asked about them again. An editor extension that runs several servers (ex. one for each project, or for each `dprint` executable in a window) sends it to each server that may have used the configuration file (or to each of them without a `configFile`), because a server only forgets the decisions of its own session, and forgets what it asked about for each. A download that was allowed stays allowed for the session, and a plugin that's in dprint's cache doesn't need to be downloaded again. It responds once the configuration files were resolved again, and sends `dprint/configChanged` for the ones whose results changed. It forgets the decisions of this server's session and the stored ones, including an `always` decision this server is still storing (ex. while another process holds the approvals file's lock), which it waits for, but not those of another server's session: an `alwaysAllow` of process plugins that weren't set up that another server received (ex. while what they need downloaded wasn't allowed yet) is only stored once that server sets them up, so it's still stored then, because it's what the user decided there.

```ts
interface ForgetDecisionsParams {
  /**
   * The configuration file to forget the decisions of, which is every configuration file when not provided. Without a `dirUri`, it's
   * the configuration file for each directory it's used for (ex. the global configuration file on each drive).
   */
  configFile?: ConfigFileIdentifier;
  workDoneToken?: string | number;
}

interface ForgetDecisionsResult {
  /** The decisions that were forgotten. */
  decisions: Decision[];
  /**
   * What to tell the user about forgetting them (ex. the stored decisions couldn't be forgotten, which includes when the approvals
   * file is from a newer version of dprint that this one can't write to, so only the session's decisions were, or the file of the
   * session couldn't be written or deleted, so they may come back if the server is started again, or the session's decisions aren't
   * kept yet because another server that's running uses the session id or its lock can't be taken).
   */
  notice?: Notice;
}
```

### `dprint/decisions` (request)

Gets what the user decided about the approvals of a configuration file, or of every configuration file (ex. for a command that shows them). A process plugin of a configuration file has the decision that's used for it, which is the most recent one. A decision that allows it may be for an earlier configuration of the plugin, which is then asked about again when it's used instead of an older decision being used, while a decision that denies it is for any configuration.

```ts
interface DecisionsParams {
  /**
   * The configuration file to get the decisions of, which is every configuration file when not provided. Without a `dirUri`, it's
   * the configuration file for each directory it's used for (ex. the global configuration file on each drive).
   */
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
   * The configuration file it's for, which can be sent to `dprint/forgetDecisions` to forget the decisions of that configuration
   * file. It's missing for a download that's allowed for every configuration file, and for a process plugin decision whose
   * configuration file isn't a path or url the server can make a uri of (ex. one stored on another operating system). A decision
   * without one can't be forgotten on its own (an allowed download stays allowed for the session), so don't offer to forget it,
   * and never send `dprint/forgetDecisions` without a `configFile` for it, which forgets every decision of every configuration
   * file. Its `dirUri` is missing when it's for every directory the configuration file is used for (ex. the global
   * configuration file).
   */
  configFile?: ConfigFileIdentifier;
  /** The urls, paths, or specifiers it's for, like the `specifier` of an approval's items and escaped the same way (ex. the process plugins, or the setting of an `.npmrc` proxy). */
  specifiers: string[];
  /**
   * What it's for, like an approval's items, with one for each of the `specifiers` in the same order. The server usually provides it for
   * downloads so a proxy or a path on another machine can be told apart from a url and shown with its file (an `npmrcProxy` with its
   * `npmrcUri`, or a `networkPath` with its `localUri`), and it's missing when they aren't known (ex. a denial kept by another version of
   * dprint). Show the `specifiers` when it's missing.
   */
  items?: ApprovalItem[];
  allows: boolean;
  /** Whether it's remembered after the session ends, which includes running dprint with the configuration file (ex. `dprint fmt`). */
  always: boolean;
}
```

### `dprint/reset` (request)

Forgets the configuration files and stops the plugins, which are set up again when they're next used (ex. after a plugin stopped responding or was changed). Unlike the restart command (a new server with a new `sessionId`), what the user decided for the session is kept. It takes `{}` and its result is `{}`. Get the plugins of the configuration files again afterwards because a plugin that changed may format other files.

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
- The server keeps what the user decided for the session in a file for the client's session id, instead of the client sending the session's decisions again to a server that's started again. The clients kept getting the bookkeeping for that wrong (which ids an approval had before, which decisions replace each other, and whether an `always` decision was stored, including one stored once its process plugins were set up by a later approval), which silently allowed or denied what the user decided otherwise since. The server already knows each of these, so a file it writes after each change makes a server that's started again behave like the one that exited, and a client only keeps which approvals it asked about. The file is per session id, which is unique to a server process, so servers of different sessions don't share a lock. Two servers that are running with the same session id are a mistake of the client (or a server that's started again before the previous one exited), so the second one doesn't read or write the file until the first one exited, which its lock of the session tells, and then writes its own decisions to it right away, so the file never has the decisions of the other server. A server that's started again after a crash also leaves out the session's process plugin allows and set-up grants when the approvals file changed since the server that exited last read it (or that isn't known), because a stored decision that replaced them may have been removed since (ex. an `alwaysDeny` that was then forgotten in another window), which a server that kept running would have seen. It keeps the allowed downloads, because the approvals file has no download decisions. The id only selects the file in the user's cache directory, so it can't point the server at a file of the project.
- A decision isn't remembered by url or domain beyond the session, and allowing a download only allows its urls, because a broader scope (ex. trusting a domain) is the settings file's `editor.trustedDomains`, which the user edits on purpose.
-->
