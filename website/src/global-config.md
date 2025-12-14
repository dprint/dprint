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
dprint config init --global
```

This creates a `dprint.json` file in your system's configuration directory. The default location is:

- **Linux/macOS**: `~/.config/dprint/dprint.json`
- **Windows**: `%APPDATA%\dprint\dprint.json`

You can customize the global config directory by setting the `DPRINT_CONFIG_DIR` environment variable.

## Managing the Global Configuration

Add plugins to your global configuration (alternatively use the `-g` alias instead of `--global`):

```sh
dprint config add --global typescript
```

Update plugins in your global configuration:

```sh
dprint config update --global
```

Edit your global configuration file:

```sh
dprint config edit --global
```

This last command will use the editor configured in the `DPRINT_EDITOR` environment variable, then `VISUAL`, then `EDITOR`. If none of these environment variables are set, it will launch the file in vscode if `code` exists, or default to `notepad` on Windows and `nano` elsewhere.

## Using the Global Configuration

Once setup, the global configuration will be used by default when there's no dprint configuration file in the current directory tree; however, to prevent accidentally formatting such directories, a prompt is shown when calling `dprint fmt`:

```
> dprint fmt
Warning You're not in a dprint project. Format '/home/david/dev/scratch' anyway? (Y/n) █

Hint: Specify the directory to bypass this prompt in the future (ex. `dprint fmt .`)
```

As the hint states, you can bypass this by providing the current directory:

```
> dprint fmt .
Formatted 1 file.
```

To format files using only the global configuration and ignore local configuration files use:

```sh
dprint fmt --config-discovery=global
```
