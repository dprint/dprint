---
title: Setup
description: Documentation on setting up dprint to format a collection of code.
---

# Setup dprint

After [installing](/install), the main part of getting setup is to create a _dprint.json_ or hidden _.dprint.json_ file in your project.

This file will outline:

1. The plugins to use.
2. The configuration to use for formatting files.
3. Which files to include and exclude from formatting.

## Quick Setup

Using the `dprint init` command is a quick way to get setup formatting your project.

Open a terminal in the root directory of your project and run the following command:

```bash
dprint init
```

This will create a _dprint.json_ file in the current working directory. If you are connected to the internet, it will initialize the file according to the latest plugins.

## Manual Setup

Create a _dprint.json_ or hidden _.dprint.json_ file in the root directory of the project and read the [configuration documentation](/config).

## Hidden Config File

The dprint CLI supports a default hidden configuration at _.dprint.json_.

## Custom Config File Location

It is recommended to use _dprint.json_ or _.dprint.json_ as the location of your configuration file because it will be automatically picked up by the CLI and editor plugins. If you place it in another other location then it will need to be manually specified using the `--config <path>` or `-c <path>` flag whenever you run a command.

### `dprint init` with custom config file location

You may specify a custom path for the creation of a configuration file via `dprint init` by specifying it with the `-c` or `--config` flag.

```bash
dprint init --config .dprint.json
dprint init --config path/to/dprint.json
```

## Proxy

You may specify a proxy for dprint to use when downloading plugins or configuration files by setting the `HTTPS_PROXY` and `HTTP_PROXY` environment variables.

Next step: [Configuration](/config)
