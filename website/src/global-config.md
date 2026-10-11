---
title: Global Configuration
description: Documentation on global configuration.
layout: layouts/documentation.njk
---

# Global Configuration

Starting in dprint 0.51, you can maintain a global configuration file that applies formatting config when the current folder does not have a local dprint configuration file.

## Initializing a Global Configuration

Create a global configuration file by running:

```sh
dprint init --global
```

This creates a `dprint.jsonc` file in your system's configuration directory. The default location is:

- **Linux/macOS**: `~/.config/dprint/dprint.jsonc` (or `$XDG_CONFIG_HOME/dprint/dprint.jsonc`)
  - Note: On macOS, it will use the config file in `$HOME/Library/Application Support/dprint` if that folder exists.
- **Windows**: `%APPDATA%\dprint\dprint.jsonc`

You can customize the global config directory by setting the `DPRINT_CONFIG_DIR` environment variable.

## Managing the Global Configuration

Add plugins to your global configuration (alternatively use the `-g` alias instead of `--global`):

```sh
dprint add --global typescript
```

Download and set up the plugins in your global configuration ahead of time (provide plugins to add them first, ex. `dprint install --global typescript`):

```sh
dprint install --global
```

Update plugins in your global configuration:

```sh
dprint config update --global
```

Edit your global configuration file:

```sh
dprint config edit --global
```

The editor to use for the global config follows the same rules as [Editing Config via CLI](/config#editing-config-via-cli) (set the `DPRINT_EDITOR` environment variable to customize it)

## Using the Global Configuration

Once setup, the global configuration will be used by default when there's no dprint configuration file in the current directory tree; however, to prevent accidentally formatting such directories, a prompt is shown when calling `dprint fmt`:

```
> dprint fmt
Warning You're not in a dprint project. Format '/home/david/dev/scratch' anyway? (Y/n) █

Hint: Specify the directory to bypass this prompt in the future (ex. `dprint fmt .`)
```

As the hint states, you can bypass the confirmation prompt by providing the current directory:

```
> dprint fmt .
Formatted 1 file.
```

To format files using only the global configuration and ignore local configuration files use:

```sh
dprint fmt --config-discovery=global
```

## Editors

### Language Server

Editor integrations that use the [language server](/lsp) (`dprint lsp`) format a file with the closest dprint configuration file in its directory or its ancestor directories. Starting in dprint 0.60, a file without one is not formatted by default. To format these files with the global configuration, either enable the server's `useGlobalConfig` setting or run its `dprint.formatWithGlobalConfig` or `dprint.formatSelectionWithGlobalConfig` command to format the current file once. See [Language Server - Global Configuration](/lsp#global-configuration) for more details.

### Visual Studio Code

The [Visual Studio Code extension](https://marketplace.visualstudio.com/items?itemName=dprint.dprint) doesn't format with the global configuration by default. This prevents accidentally formatting files in a project that doesn't use dprint (ex. when format on save is enabled).

Starting in version 0.18 of the extension, a file is formatted with the closest dprint configuration file in its directory or its ancestor directories. This includes files that aren't in a workspace folder, such as a single file opened on its own. When there's no such configuration file, the file is not formatted and the global configuration is used in one of two ways:

1. Set `dprint.useGlobalConfig` to `true` to format these files with the global configuration:

   ```jsonc
   {
     "dprint.useGlobalConfig": true
   }
   ```

   As with the rest of the extension's formatting, this only applies to the languages where dprint is the default formatter (`editor.defaultFormatter`).
2. Run the `Dprint: Format Document (global config)` or `Dprint: Format Selection (global config)` command from the command palette to format the current file once. These commands work regardless of the `dprint.useGlobalConfig` setting and are only shown for a file that doesn't have a configuration file in an ancestor directory. When the file isn't formatted, the command says why (ex. there's no global configuration file or none of its plugins handle the file).

The global configuration file is the same one the CLI uses, including when `DPRINT_CONFIG_DIR` is set. As with `dprint fmt`, a file is only formatted when the global configuration would format it—a plugin needs to handle the file and it can't be matched by the `excludes`.
