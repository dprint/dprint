---
title: Setup
description: Documentation on setting up dprint to format a collection of code.
layout: layouts/documentation.njk
---

# Setup dprint

After [installing](/install), the main part of getting setup is to create a _dprint.json_/_dprint.jsonc_, or hidden _.dprint.json_/_.dprint.jsonc_ file in your project.

This file will outline:

1. The plugins to use.
2. The configuration to use for formatting files.
3. Which files to include and exclude from formatting.

## Quick Setup

Using the `dprint init` command is a quick way to get setup formatting your project.

Open a terminal in the root directory of your project and run the following command:

```sh
dprint init
```

This will create a _dprint.json_ file in the current working directory. If you are connected to the internet, it will prompt you to select from the latest plugins, pre-selecting the ones that match the files found in the current directory. Use the spacebar to toggle a plugin, type to filter the list, and press enter when finished.

### Non-interactive init

Pass the `--yes` or `-y` flag to skip the prompt and accept the plugins selected based on the files in the current directory. This is useful in scripts or CI:

```sh
dprint init --yes
```

The prompt is also skipped automatically when there is no interactive terminal.

## Manual Setup

Create a _dprint.json_/_dprint.jsonc_ or hidden _.dprint.json_/_.dprint.jsonc_ file in the root directory of the project and read the [configuration documentation](/config).

## Hidden Config File

The dprint CLI supports a default hidden configuration at _.dprint.json_ or _.dprint.jsonc_.

## Custom Config File Location

It is recommended to use an auto-discoverable dprint configuration file name (ex. _dprint.json_) as the location of your configuration file because it will be automatically picked up by the CLI and editor plugins. If you place it in another other location then it will need to be manually specified using the `--config <path>` or `-c <path>` flag whenever you run a command.

### `dprint init` with custom config file location

You may specify a custom path for the creation of a configuration file via `dprint init` by specifying it with the `-c` or `--config` flag.

```sh
dprint init --config .dprint.jsonc
dprint init --config path/to/dprint.json
```

## Global Config File

See [global configuration](/global-config)

## Custom Cache Directory

By default, dprint stores information in the current system user's cache directory (`~/.cache/dprint` on Linux, `~/Library/Caches/dprint` on Mac, and `%LOCALAPPDATA%/dprint` on Windows) such as cached plugins and incremental formatting information. If you would like to store the cache in a custom location, then specify a `DPRINT_CACHE_DIR` environment variable. Note that this directory may be periodically deleted by the CLI, so if you set it please make sure it's set correctly and you're ok with the custom directory being deleted.

## Proxy

You may specify a proxy for dprint to use when downloading plugins or configuration files by setting the `HTTPS_PROXY`/`https_proxy` and `HTTP_PROXY`/`http_proxy` environment variables.

Additionally, the `NO_PROXY`/`no_proxy` environment variable can be set, which is a comma-separated list of hosts which should not use the proxy.

## TLS Certificates

dprint downloads plugins via HTTPS. In some cases you may wish to configure this. This is possible via the following environment variables:

- `DPRINT_CERT` - Load certificate authority from PEM encoded file.
- `DPRINT_TLS_CA_STORE` - Comma-separated list of order dependent certificate stores.
  - Possible values: `mozilla` and `system`
  - Defaults to `mozilla,system`

Requires dprint >= 0.46.0

### Unsafely ignoring certificates

You can unsafely ignore all or some TLS certificates via the `DPRINT_IGNORE_CERTS` environment variable:

- `DPRINT_IGNORE_CERTS=1` - Ignore all TLS certificates.
- `DPRINT_IGNORE_CERTS=dprint.dev,localhost,[::],127.0.0.1` - Ignore certs from the specified hosts.

This is very unsafe to do and not recommended. A warning will be displayed on first download when this is done.

## Limiting Parallelism

By default, dprint only runs for a short period of time and so it will try to take advantage of as many CPU cores as it can. This might be an issue in some scenarios, and so you can limit the amount of parallelism by setting the `DPRINT_MAX_THREADS` environment variable in version 0.32 and up (ex. `DPRINT_MAX_THREADS=4`).

Separately, dprint reads directories on several threads when discovering files in order to better saturate the disk. This is I/O bound, so the number of read threads is independent of `DPRINT_MAX_THREADS`. You can override it with the `DPRINT_GLOB_READ_THREADS` environment variable (ex. `DPRINT_GLOB_READ_THREADS=8`), though the default is suitable for most setups.

Next step: [Configuration](/config)
