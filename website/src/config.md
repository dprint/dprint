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
dprint config add
```

This will prompt asking you which of the standard plugins you want to add.

Alternatively, specify the specific name of the plugin to add based on its GitHub repo:

```sh
dprint config add dprint/dprint-plugin-typescript
```

Or for the standard plugins, you can just do:

```sh
dprint config add typescript
```

Or specify a plugin url:

```sh
dprint config add https://plugins.dprint.dev/json-x.x.x.wasm
```

### Updating Plugins via CLI

Plugins can be updated to the latest version in the configuration file by running:

```sh
dprint config update
```

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
    // will format dist/main.js even though it's gitignored
    "!dist/main.js"
  ]
}
```

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

### Excluding paths from plugin

Only providing negated globs as an association can be a way to exclude a file extension or path from being formatted with a certain plugin, but continue using file extensions to match a plugin otherwise.

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

## Plugin/Language Specific Configuration

Running `dprint help` will list the help urls for all the configured plugins in your configuration file. On those pages you can view the help information.

For information on the official plugins' configuration, see the [plugins](https://dprint.dev/plugins/) section.

Next step: [CLI](/cli)
