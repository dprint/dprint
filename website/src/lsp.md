---
title: Language Server
description: Documentation on dprint's language server (dprint lsp) for formatting in editors.
layout: layouts/documentation.njk
---

# Language Server

The `dprint lsp` subcommand starts a server that formats code over the [language server protocol](https://microsoft.github.io/language-server-protocol/) (LSP). It's how to use dprint in an editor that doesn't have a dedicated dprint extension, such as Neovim, Helix, or Zed.

```sh
dprint lsp
```

The server communicates over stdin and stdout, so this command is run by the editor rather than by you. See [Editor Setup](#editor-setup) for how to tell an editor to run it.

`dprint lsp` was added in dprint 0.45. As with the rest of the documentation, this page describes the latest version, so upgrade (ex. `dprint upgrade`) if something here doesn't work for you.

## Features

- **Formatting** of a whole document (`textDocument/formatting`).
- **Range formatting** (`textDocument/rangeFormatting`). The range is passed on to the plugin, so what's formatted is up to the plugin.
- **Completions and hover information in dprint configuration files** (files named `dprint.json`, `dprint.jsonc`, `.dprint.json`, or `.dprint.jsonc`). This covers dprint's own properties and the configuration of the plugins listed in the saved file.
- **Notebook cells** and **untitled documents**, in clients that support them (see below).
- **Commands for formatting with the global configuration file** (see [Global Configuration](#global-configuration)).

The server only formats. It doesn't provide diagnostics or code actions.

### Configuration file completions and hover

Completions and hover information for dprint's own properties (ex. `plugins`, `excludes`, `lineWidth`) come from a schema that's built into the dprint executable, so they work offline.

For a plugin's configuration (ex. the properties within `"typescript": { ... }`), the server downloads the JSON schema each plugin in the configuration file provides. The plugins are read from the configuration file on disk and not from the editor's unsaved text, so save the file after adding a plugin to get completions for it. A request waits at most a couple of seconds on these downloads, so the first completions in a file may be missing a plugin's properties until its schema has downloaded. A schema that fails to download is tried again later.

In a client that supports dynamic registration of completion and hover, the server only registers these for dprint configuration files. In other clients, the server has to advertise them for every document the client uses the server for, but it answers with nothing outside of a dprint configuration file.

### Notebook cells

The server formats the cells of a notebook that's on the file system when the client supports notebook document synchronization (`notebookDocument/*`) and the dynamic registration of formatting. This is mostly clients built on VS Code's language client.

A cell is only formatted when the notebook itself would be formatted by `dprint fmt`—a plugin in the configuration formats the notebook file (ex. the [Jupyter plugin](/plugins/jupyter)) and the notebook isn't excluded. The cell is then formatted by the plugin that handles its language. For example, a Python cell in `notebooks/analysis.ipynb` is formatted as if it was the file `notebooks/code_block.py`.

### Untitled documents

A new document that hasn't been saved yet has a URI with the `untitled` scheme in VS Code-style clients. The server asks clients that support dynamic registration of text document synchronization and formatting to send it these documents.

Since an untitled document doesn't have a file path, it's formatted as a file named `Untitled.<ext>` in the first workspace folder, or in the home directory when there is no workspace folder. The extension is based on the document's language (ex. `Untitled.ts` for `typescript`), and the configuration file is [resolved](#configuration-file-resolution) for that path.

Documents with other schemes that aren't files on the file system are not formatted.

## Configuration File Resolution

The server doesn't use a single configuration file for a workspace. On each request it resolves the configuration file for the file being formatted:

1. The closest `dprint.json`, `dprint.jsonc`, `.dprint.json`, or `.dprint.jsonc` in the file's directory or its ancestor directories is used.
2. When that configuration file specifies [`"inherit": true`](/config#directory-specific-configuration), it's merged with the next configuration file found in its ancestor directories, the same as the CLI does. That one may also inherit, and when there's none in an ancestor directory the global configuration file is inherited from when [using the global configuration](#global-configuration).
3. When no configuration file is found in an ancestor directory, the [global configuration file](/global-config) is used when one exists and the server is set to [use it](#global-configuration).
4. Otherwise the file is not formatted and you're [notified](#no-configuration-file-notification) the first time that happens in a session.

This means files in different projects, or in directories of a monorepo with their own configuration file, are each formatted with their own configuration in the same editor session.

A file is then only formatted when the configuration file would have `dprint fmt` format it—the file needs to match the configuration's `includes` and a plugin, and not be matched by its `excludes` or a `.gitignore` file. Otherwise the server responds to the editor with no edits.

Configuration files are read on each request, so a change to one is used the next time a file is formatted without needing to restart the server. Remote configuration files they extend are [cached](/config#remote-configuration-caching) though, and `dprint lsp --reload` has the server check for newer versions of them whenever a configuration file changes.

Note: The language server does not use the config discovery mode. The `--config-discovery` flag and the `DPRINT_CONFIG_DISCOVERY` environment variable described in [changing config discovery](/cli#changing-config-discovery) have no effect on `dprint lsp`. The `--plugins` flag has no effect either. The server only uses the plugins in the configuration file.

## Global Configuration

A file without a configuration file in its directory or ancestor directories is not formatted by default. Before dprint 0.60, the server formatted these files with the [global configuration file](/global-config) without being asked to. There are two ways to format them with the global configuration file:

1. Enable the `useGlobalConfig` [setting](#settings) to always format these files with the global configuration. This is what makes formatting work in a scratch directory or in a project that doesn't use dprint.
2. Run one of the following commands (`workspace/executeCommand`) to format the current file once. They work regardless of the `useGlobalConfig` setting, and the server applies the edits with `workspace/applyEdit`. The edits are for the version of the document that was formatted, so a client that supports versioned document edits (`workspace.workspaceEdit.documentChanges`) rejects them when the document changed in the meantime:

   - `dprint.formatWithGlobalConfig` formats a document. Its only argument is the document's URI.
   - `dprint.formatSelectionWithGlobalConfig` formats a range of a document. Its arguments are the document's URI and the range to format (an LSP `Range`).

   A file with a configuration file in an ancestor directory is still formatted with that configuration file. When the file isn't formatted, the server shows a message saying why (ex. there's no global configuration file or none of its plugins handle the file), unless the file is already formatted.

How to run a command of a language server depends on the editor. Not every editor has a way to.

A client may also format a single request with the global configuration by setting `useGlobalConfig` to `true` in the formatting options of a `textDocument/formatting` or `textDocument/rangeFormatting` request.

### No configuration file notification

The first time in a session that a file isn't formatted because no configuration file was found for it, the server shows a message saying so. In clients that have the `window.showMessage` capability, the message is shown with `window/showMessageRequest` and has two actions:

- `Don't show in this workspace` stops showing it for the current workspace folders. This is only offered when there's a workspace folder.
- `Don't show again` stops showing it everywhere.

A server can't change an editor's settings, so the choice is stored in an `lsp-state.json` file in dprint's cache directory. Running `dprint clear-cache` or deleting that file shows the notification again. To turn off the notification from the editor instead, set the `showNoConfigNotification` [setting](#settings) to `false`.

## Settings

The server reads the following settings from the `initializationOptions` of the `initialize` request and from `workspace/didChangeConfiguration` notifications, where they may be at the top level of the object or within a `dprint` property:

- `useGlobalConfig` (default: `false`) - Format the files that don't have a configuration file in an ancestor directory with the [global configuration file](#global-configuration).
- `showNoConfigNotification` (default: `true`) - Show the [no configuration file notification](#no-configuration-file-notification).

For example, in Neovim (as with the snippets in [Editor Setup](#editor-setup), these were written from each editor's documentation and have not been tested in those editors):

```lua
vim.lsp.config("dprint", {
  init_options = { useGlobalConfig = true },
})
```

Or in Helix's `languages.toml`:

```toml
[language-server.dprint]
command = "dprint"
args = ["lsp"]
config = { useGlobalConfig = true }
```

For an editor that can't provide settings to the server, setting the `DPRINT_EDITOR_USE_GLOBAL_CONFIG` environment variable to `1` or `true` changes the default of `useGlobalConfig` to `true`.

## Line Endings

The server keeps the line endings of the editor's document. When a plugin formats the text with different line endings than the document has (ex. because of the `newLineKind` configuration), the server converts them back to the kind used by the first line ending in the document before computing the edits. A document that has no line endings (a single line) is the exception because there's nothing to match, so it gets the line endings the plugin produced.

To change a file's line endings, change them in the editor or run `dprint fmt` from the command line.

## Formatting Options

The formatting options an editor sends with a format request (`tabSize`, `insertSpaces`, etc.) are ignored. Indentation and everything else comes from the dprint configuration file. The exception is the `useGlobalConfig` option described in [Global Configuration](#global-configuration).

## Troubleshooting

The server doesn't respond to a format request with an error or show a popup about it, because a file that can't be formatted (ex. one with a syntax error) would then interrupt you each time you format on save. A request that fails or does nothing gets a response without edits, and the reason is logged instead. The only popups (`window/showMessage` and `window/showMessageRequest`) are the once per session [no configuration file notification](#no-configuration-file-notification) and the result of a [command](#global-configuration) you explicitly ran.

The following is logged to the client with `window/logMessage`, which most editors show in a language server log or output panel:

- The dprint version and `Server ready.` once the server is initialized (info).
- `Failed formatting '<uri>': <error>` when formatting fails (error). This includes a plugin failing to parse the file, a configuration file that fails to resolve, and a plugin's configuration diagnostics.
- Why a document can't be formatted (warning). For example when it's not a file, untitled document, or cell of an open notebook; when a file path can't be determined for the language of an untitled document or notebook cell; when the client didn't open the document; or when the range to format is invalid.
- A failure to register capabilities with the client (warning).

Some things are only written to the server's stderr, which is where the server's other logging goes:

- `Path did not have a dprint config file: <path>` when no configuration file was resolved for a file.
- `[DEBUG] Excluded file: <path>` when the file isn't matched by the configuration's `includes` or is matched by its `excludes` or a `.gitignore` file. A file that no plugin handles isn't logged this way. It shows up as a format request with an empty list of plugins. These and the timing of each format request are only logged with `dprint lsp --log-level=debug`.

When a file isn't being formatted in the editor, check that `dprint fmt` formats it from the command line and see [diagnostic commands and flags](/cli#diagnostic-commands-and-flags).

## Editor Setup

The Visual Studio Code and IntelliJ extensions listed on the [install page](/install#editor-extensions) are set up on their own and don't require the steps below.

For other editors, the general steps are:

1. [Install dprint](/install) so that `dprint` is on the path.
2. Configure the editor to run `dprint lsp` for the languages your dprint plugins format, and for JSON/JSONC if you want completions in dprint configuration files.
3. If the language has another language server that also formats, tell the editor which one to format with.

The snippets below were written from each editor's documentation and have not been tested by the dprint maintainers in those editors. Please [open an issue](https://github.com/dprint/dprint/issues) or a pull request if one needs correcting.

### Neovim

[nvim-lspconfig](https://github.com/neovim/nvim-lspconfig) has a [`dprint` configuration](https://github.com/neovim/nvim-lspconfig/blob/master/doc/configs.md#dprint) that runs `dprint lsp`. With nvim-lspconfig installed on Neovim 0.11 or later, enable it with:

```lua
vim.lsp.enable("dprint")
```

That configuration only attaches to a fixed list of file types (JavaScript, TypeScript, JSON, Markdown, Python, TOML, Rust, and a few others at the time of writing). To change the file types, override that setting. Note that `filetypes` replaces the default list instead of adding to it, so list every file type you want the server to attach to:

```lua
vim.lsp.config("dprint", {
  filetypes = { "javascript", "typescript", "json", "jsonc", "markdown", "yaml", "css" },
})
vim.lsp.enable("dprint")
```

Then format with `vim.lsp.buf.format()`.

### Helix

Helix doesn't have a built-in entry for dprint, so define the language server in `languages.toml` and add it to each language to format with it (see Helix's [language configuration documentation](https://docs.helix-editor.com/languages.html)):

```toml
[language-server.dprint]
command = "dprint"
args = ["lsp"]

[[language]]
name = "typescript"
language-servers = [
  { name = "typescript-language-server", except-features = ["format"] },
  "dprint",
]
auto-format = true

[[language]]
name = "markdown"
language-servers = ["dprint"]
auto-format = true
```

Setting `language-servers` replaces the language's default language servers, so list the ones you want to keep and use `except-features = ["format"]` on any that would otherwise format instead of dprint.

### Zed

Zed needs an extension to start a language server it doesn't ship with. There is a community maintained [Dprint extension](https://zed.dev/extensions/dprint) ([source](https://github.com/panikkastudio/dprint-zed)) that runs `dprint lsp` with the language server name `dprint`. It's not maintained by the dprint project.

After installing the extension, select dprint as the formatter of a language in `settings.json`:

```json
{
  "languages": {
    "TypeScript": {
      "formatter": [{ "language_server": { "name": "dprint" } }],
      "format_on_save": "on"
    }
  }
}
```

According to the extension's documentation, it uses the `dprint` executable in `node_modules/.bin` or on the path (downloading one otherwise) and the executable and its arguments can be changed with the `lsp.dprint.binary` setting:

```json
{
  "lsp": {
    "dprint": {
      "binary": {
        "path": "/path/to/dprint",
        "arguments": ["lsp"]
      }
    }
  }
}
```
