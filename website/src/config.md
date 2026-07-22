---
title: Configuration
description: Documentation on dprint's configuration file.
layout: layouts/documentation.njk
---

# Configuration

Configuration allows you to modify how dprint and its plugins behave.

## Setup

See [Setup](/setup).

## Example

```json
{
  "lineWidth": 80,
  "typescript": {
    // This applies to both JavaScript & TypeScript
    "quoteStyle": "preferSingle",
    "binaryExpression.operatorPosition": "sameLine"
  },
  "json": {
    "indentWidth": 2
  },
  "excludes": [
    "**/*-lock.json"
  ],
  "plugins": [
    // You may specify any urls or file paths here that you wish.
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/markdown-x.x.x.wasm"
  ]
}
```

## Plugins

The `plugins` property specifies which plugins to use for formatting. These may be URLs or file paths to a WebAssembly file of the plugin.

```json
{
  // ...omitted...
  "plugins": [
    // You may specify any urls or file paths here that you wish.
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/markdown-x.x.x.wasm"
  ]
}
```

Alternatively, these may be provided to the CLI via the `--plugins <plugin urls or file paths...>` flag.

Note: The order of the plugins in this array defines the precedence. If two plugins support the same file extension then define the one you want to format that extension with first. For more fine grained control, see "associations" below.

### Adding Plugins via CLI

You may add a plugin via the CLI by running:

```sh
dprint add
```

This will prompt asking you which of the standard plugins you want to add.

Alternatively, specify the specific name of the plugin to add based on its GitHub repo:

```sh
dprint add dprint/dprint-plugin-typescript
```

Or for the standard plugins, you can just do:

```sh
dprint add typescript
```

You can also add multiple plugins at once:

```sh
dprint add typescript json markdown
```

Or specify a plugin url:

```sh
dprint add https://plugins.dprint.dev/json-x.x.x.wasm
```

Or from npm:

```sh
dprint add npm:@dprint/json
```

By default Wasm plugins are added without a checksum (process plugins always get one). Use `--checksum` to pin a checksum on the added plugin regardless:

```sh
dprint add --checksum typescript
```

Note: `dprint config add` also works and is equivalent.

### Updating Plugins via CLI

Plugins can be updated to the latest version in the configuration file by running:

```sh
dprint config update
```

To update configuration files in descendant directories, run `dprint config update --recursive`.

To preview the updates that would be made without modifying any files, run `dprint config update --dry-run`.

### Using Plugins from npm

Plugins can be referenced by npm specifiers instead of HTTPS URLs. This is useful in environments where downloading from `plugins.dprint.dev` or other domains is restricted, or when you're already using npm and want one place to download dependencies from.

Supported `npm:` specifier forms:

```jsonc
{
  "plugins": [
    // resolve a pinned version from the npm registry (wasm plugin)
    "npm:@dprint/typescript@0.95.15",

    // process plugin — requires a tarball checksum
    "npm:@dprint/prettier@0.50.0/plugin.json@<sha256>",

    // resolve from your local node_modules, walking up from the config file
    // (use this when an npm package manager manages the version)
    "npm:@dprint/json",
  ],
}
```

Behaviour:

- A version after `@` makes dprint fetch the package from the npm registry and cache it. Wasm plugins don't need a checksum; process plugins do (the `@<sha256>` after the path).
- Omitting the version (`npm:@scope/name`) tells dprint to look up the package in `node_modules` walking up from the config file's directory. Use this when you want npm and your lockfile to be the source of truth.
- The registry is resolved from `NPM_CONFIG_REGISTRY`, then `.npmrc` files walking up from the config, then `~/.npmrc`, then the default `https://registry.npmjs.org`. Scoped registries are supported.
- `dprint config update` will bump versioned npm specifiers to the latest published version (and compute the new checksum for process plugins). Unversioned specifiers are managed by your package manager, so they're skipped.
- `dprint add npm:@scope/name` resolves to the latest version and writes the pinned form, unless the package is listed in a nearby `package.json` under `devDependencies` — in which case the unversioned form is written so npm/`package-lock.json` stays the source of truth. When you don't include a plugin path, dprint inspects the package to detect whether it's a Wasm or process plugin and writes the right form automatically — for a process plugin that means `npm:@scope/name@<version>/plugin.json@<sha256>`.

Available npm packages include `@dprint/typescript`, `@dprint/json`, `@dprint/markdown`, `@dprint/toml`, `@dprint/dockerfile`, `@dprint/biome`, `@dprint/oxc`, `@dprint/ruff`, `@dprint/sql`, `@dprint/mago`, `@dprint/jupyter`, `@dprint/exec`, `@dprint/prettier`, and `@dprint/roslyn`.

You can also reference a plugin file directly in `node_modules` if you prefer (`"./node_modules/@dprint/typescript/plugin.wasm"`); the `npm:` form just removes the need for that path.

#### Private npm registries

dprint reads `.npmrc` files (walking up from the config, then `~/.npmrc`) to pick a registry and credentials. Both common auth schemes are supported:

```
@mycorp:registry=https://npm.mycorp.com
//npm.mycorp.com/:_authToken=${MYCORP_NPM_TOKEN}
```

- `_authToken=…` → sent as `Authorization: Bearer …`
- `_auth=…` (already base64-encoded user:pass) → sent as `Authorization: Basic …`
- `${VAR}` substitution works the same as for npm itself.

Credentials are dropped on cross-origin redirects (e.g. a registry that redirects tarball downloads to a CDN on a different host), so they never leak outside the configured registry.

#### Process plugins distributed via npm

A process plugin's `plugin.json` lists per-platform binaries; for npm-installed process plugins the `reference` field must be one of:

- a relative path (`"./bin.zip"`) or `file:///…` URL — resolved against the plugin.json's directory inside the npm package. This is the simplest layout: ship `plugin.json` and `bin.zip` together in one npm package.
- an `npm:` specifier — used when the per-platform binary lives in a separate npm package (e.g., the manifest references `npm:@scope/foo-linux-x86_64@1.0.0/plugin.zip` and the dep is installed alongside via npm `optionalDependencies`).

`http://` and `https://` references are rejected for npm-installed process plugins. The whole point of installing via npm is to avoid surprise network fetches at format time, so the binary must come from the npm package(s) you already installed.

#### Cache layout

Resolved npm packages are extracted under `<dprint-cache>/npm/<registry-host>/<name>@<version>/`. Different registries (public vs. private mirror) get separate directories. Compiled wasm modules and extracted process-plugin binaries continue to live under `<dprint-cache>/plugins/`. `dprint clear-cache` wipes everything.

### Editing Config via CLI

```sh
dprint config edit
```

Editing the configuration file will use the editor configured in the `DPRINT_EDITOR` environment variable, then `VISUAL`, then `EDITOR`. If none of these environment variables are set, it will launch `notepad` on Windows and `nano` elsewhere.

## Excludes

The `excludes` property specifies the file paths exclude from formatting.

These should be file globs according to [`gitignore`'s extended glob syntax](https://git-scm.com/docs/gitignore#_pattern_format):

```json
{
  // ...omitted...
  "excludes": [
    "**/*-lock.json"
  ]
}
```

### Un-excluding gitignored files

Files that are gitignored will be excluded by default, but you can "un-exclude" them by specifying a negated glob:

```json
{
  "excludes": [
    // will format dist.js even though it's gitignored
    "!dist.js"
  ]
}
```

Alternatively, you can disable all `.gitignore` handling with the `--no-gitignore` CLI flag (see [CLI docs](/cli#ignoring-gitignore)).

### Escaping glob characters

To match a path that contains glob characters (ex. `[` or `{`) in `includes`/`excludes`, wrap each one in a character class:

```json
{
  "excludes": [
    // excludes a file named {{myfile}}.json
    "[{][{]myfile[}][}].json",
    // excludes a directory named [id]
    "routes/[[]id[]]"
  ]
}
```

This is only necessary in the config file—a CLI argument that names an existing file or directory is matched literally.

## Includes

The `includes` property can be used to limit dprint to only formatting certain files. Generally, you don't need to bother providing this.

```json
{
  // ...omitted...
  "includes": [
    "src/**/*.{ts,tsx,js,jsx,json}"
  ]
}
```

## Associations

By default, plugins will pull in files based on their extension. Sometimes a file may have a different extension or no extension at all, but you still want to format it with a certain plugin. The plugin `"associations"` config allows you to do that by associating a certain file pattern to one or multiple plugins.

For example:

```json
{
  "json": {
    "associations": [
      // format any file named `.myconfigrc` matched by the
      // includes/excludes patterns in any directory with
      // the json plugin
      ".myconfigrc",
      // format this specific file using the json plugin
      "./my-relative-path/to-file",
      // format files that match this pattern
      "**/*.myconfig"
    ]
  },
  "plugins": [
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

Note that first the `"includes"`/`"excludes"` file resolution occurs and then the associations is used to map those files to a plugin. Specifying associations may also be useful for formatting a file with multiple plugins or forcing a file to be formatted with a specific plugin.

Associations are _additive_: the patterns you specify are matched **in addition to** the file extensions and file names the plugin matches by default. For example, a plugin that formats `.json` files by default will also format `.myconfig` files once `"associations": ["**/*.myconfig"]` is set—`.json` files keep being formatted. To stop matching a default extension or file name, use a negated glob (see below).

### Excluding paths from plugin

A negated glob (`!`) is the way to stop a plugin from matching a file it would otherwise format by default—whether that's a default file extension, a default file name, or a path.

In the following example, both the TypeScript plugin and Prettier plugin support formatting `.js` and `.ts` files. Say we want to only format `.ts` files with the TypeScript plugin and `.js` files with the prettier plugin. To do that, we can place the typescript plugin to have higher precedence in the "plugins" array, then add an excludes for only `!**/*.js`. This will cause the TypeScript plugin to match based on the file extension for `.ts` files, but then be excluded from matching on `.js` files.

```json
{
  "typescript": {
    "associations": [
      "!**/*.js" // don't format javascript files
    ]
  },
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    // side note: check the docs for the latest version of this plugin
    "https://plugins.dprint.dev/prettier-0.13.0.json@dc5d12b7c1bf1a4683eff317c2c87350e75a5a3dfcc127f3d5628931bfb534b1"
  ]
}
```

## Overrides

The plugin `"overrides"` config changes plugin configuration for specific files
that are already formatted by that plugin.

For example:

```json
{
  "json": {
    "overrides": {
      "files": ["**/package.json", "**/composer.json"],
      "indentWidth": 4,
      "useTabs": false
    }
  },
  "plugins": [
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

For multiple overrides, change it to an array:

```json
{
  "json": {
    "overrides": [
      {
        "files": ["**/package.json", "**/composer.json"],
        "indentWidth": 4,
        "useTabs": false
      },
      {
        "files": "**/special-package.json",
        "lineWidth": 80
      }
    ]
  },
  "plugins": [
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

Each override must specify a `"files"` pattern or list of patterns. All other
properties in the override are plugin configuration properties.

Note that `"overrides"` only changes configuration. It does not include files,
exclude files, or associate files with a plugin. File discovery still uses the
top-level `"includes"` and `"excludes"` settings, and plugin routing still uses
the plugin's supported file names, file extensions, and `"associations"`.

When multiple override blocks match the same file, they are applied in order and
later values win.

## Extending a Different Configuration File

You may extend other configuration files by specifying an `extends` property. This may be a file path, URL, or relative path (remote configuration may extend other configuration files via a relative path).

<!-- dprint-ignore -->

```json
{
  "extends": "https://dprint.dev/path/to/config/file.v1.json",
  // ...omitted...
}
```

Referencing multiple configuration files is also supported. These should be ordered by precedence:

```json
{
  "extends": [
    "https://dprint.dev/path/to/config/file.v1.json",
    "https://dprint.dev/path/to/config/other.v1.json"
  ]
}
```

Note: The `includes` property of extended _remote_ configuration is ignored for security reasons out of an abundance of caution (to disallow the dprint cli pulling in sensitive files) and additionally non-Wasm plugins are ignored in remote configuration because they don't run sandboxed.

## Directory Specific Configuration

Useful for monorepos, you may place additional configuration files in descendant directories. When dprint searches for files to format, it stops descending into a directory once it discovers a configuration file there and uses that configuration file for the files in that subtree instead (see [changing config discovery](/cli#changing-config-discovery)).

By default a nested configuration file is completely independent—it does not pick up the plugins or configuration of the ancestor configuration file:

<!-- dprint-ignore -->

```json
// ./sub-project/dprint.json
{
  // only TOML files in ./sub-project will be formatted
  "plugins": [
    "https://plugins.dprint.dev/toml-x.x.x.wasm"
  ]
}
```

To instead inherit the plugins and configuration of the ancestor configuration file, specify `"inherit": true`:

<!-- dprint-ignore -->

```json
// ./sub-project/dprint.json
{
  "inherit": true,
  "typescript": {
    // inherits the ancestor's TypeScript config, but overrides the indent width
    "indentWidth": 2
  }
}
```

When inheriting:

- Plugins specified in the nested configuration file have precedence over the ancestor's plugins. Any plugins not specified are inherited from the ancestor.
- Plugin configuration is merged with the nested configuration file winning on conflicts.
- The ancestor's `excludes` are combined with the nested configuration file's `excludes`.
- The ancestor's `includes` are _not_ inherited.

Inheriting is opt-in (rather than opt-out) so that adding a configuration file higher up in the directory structure does not unexpectedly start affecting a nested configuration file.

## Incremental

By default, dprint will only format files that have changed since the last time you formatted the code in order to drastically improve performance.

If you want to disable this functionality, you may specify the following in your dprint configuration file:

```json
{
  // etc...
  "incremental": false
  // etc...
}
```

Alternatively, specify `--incremental=false` on the CLI:

```sh
dprint fmt --incremental=false
```

## Global Configuration

There are certain non-language specific configuration that can be specified. These are specified on the main configuration object, but can be overridden on a per-plugin basis.

For example:

```json
{
  "lineWidth": 160,
  "useTabs": true,
  "typescript": {
    "lineWidth": 80
  },
  "json": {
    "indentWidth": 2,
    "useTabs": false
  },
  "plugins": [
    // etc...
  ]
}
```

Note: dprint does not provide or enforce any defaults for the global configuration. The defaults are set on a per plugin basis. When a value is not provided, the plugin may choose to use its default.

### `lineWidth`

The width of a line the formatter will try to stay under. Note that this limit will be exceeded in certain cases.

### `indentWidth`

The number of spaces for an indent when using spaces or the number of characters to treat an indent as when using tabs.

### `newLineKind`

The kind of newline to use.

- `auto` - For each file, uses the newline kind found at the end of the last line.
- `crlf` - Uses carriage return, line feed.
- `lf` - Uses line feed.
- `system` - Uses the system standard (ex. crlf on Windows).

### `useTabs`

Whether to use tabs (`true`) or spaces (`false`).

## Locking Configuration—Opinionated Configurations

You may want to publish your own opinionated configuration and disallow anyone using it from overriding the properties.

This can be done by adding a `"locked": true` property to each plugin configuration you wish to lock.

Note: When doing this, ensure you set all the global configuration values if you wish to enforce those.

### Example

Say the following configuration were published at `https://dprint.dev/configs/my-config.json`:

```json
{
  "typescript": {
    "locked": true,
    "lineWidth": 80,
    "indentWidth": 2,
    "useTabs": false,
    "quoteStyle": "preferSingle",
    "binaryExpression.operatorPosition": "sameLine"
  },
  "json": {
    "locked": true,
    "lineWidth": 80,
    "indentWidth": 2,
    "useTabs": false
  }
}
```

The following would work fine:

```json
{
  "extends": "https://dprint.dev/configs/my-config.json",
  "myOtherPlugin": {
    "propertySeparator": "comma"
  },
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/my-other-plugin-0.1.0.wasm"
  ]
}
```

But specifying properties in the `"typescript"` or `"json"` objects would cause an error when running in the CLI:

```json
{
  "extends": "https://dprint.dev/configs/my-config.json",
  "typescript": {
    "useBraces": "always" // error, "typescript" config was locked
  },
  "json": {
    "lineWidth": 120 // error, "json" config was locked
  },
  "myOtherPlugin": {
    "propertySeparator": "comma"
  },
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/my-other-plugin-0.1.0.wasm"
  ]
}
```

## Configuration Variables

Requires dprint >= 0.47.0

dprint expands certain variables in the config:

- `${configDir}` - The current configuration's directory.
- `${originConfigDir}` - The original configuration's directory. Useful when the current config is being extended by another configuration file and you want the original directory.

For example, in a JSON value you might do `"rustfmt --config-path ${configDir}/rustfmt.toml"`.

This is useful to use in some scenarios like with [dprint-plugin-exec](https://github.com/dprint/dprint-plugin-exec) because the CLI will only launch a single plugin for many configs and when resolving configs, the plugins have no concept of where that config was resolved from. Additionally, configs may resolve other configs and perhaps you want to use the directory of a configuration file that was extended.

Note: dprint will error for unknown configuration variables (ex. `"${unknown}"`). You can get around this by escaping the `$` sign (ex. `"\\${unknown}"`).

## Plugin/Language Specific Configuration

Running `dprint help` will list the help urls for all the configured plugins in your configuration file. On those pages you can view the help information.

For information on the official plugins' configuration, see the [plugins](https://dprint.dev/plugins/) section.

Next step: [CLI](/cli)
