---
title: Configuration
description: Documentation on dprint's configuration file.
---

# Configuration

Configuration allows you to modify how dprint and its plugins behave.

## Setup

See [Setup](/setup).

## Example

```json
{
  "incremental": true,
  "lineWidth": 80,
  "typescript": {
    // This applies to both JavaScript & TypeScript
    "quoteStyle": "preferSingle",
    "binaryExpression.operatorPosition": "sameLine"
  },
  "json": {
    "indentWidth": 2
  },
  "includes": [
    "**/*.{ts,tsx,js,jsx,mjs,json,md}"
  ],
  "excludes": [
    "**/node_modules",
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

Note: The order of the plugins in this array defines the precedence. If two plugins support the same file extension then define the one you want to format that extension with first.

### Adding Plugins via CLI

You may add a plugin via the CLI by running:

```bash
dprint config add
```

This will prompt asking you which of the standard plugins you want to add.

Alternatively, specify the specific name of the plugin to add which only works for certain plugins:

```bash
dprint config add dprint-plugin-typescript
```

Or specify a plugin url:

```bash
dprint config add https://plugins.dprint.dev/json-x.x.x.wasm
```

### Updating Plugins via CLI

Some plugins can be updated to the latest version in the configuration file by running:

```bash
dprint config update
```

Note that this functionality is currently very basic and only some plugins are supported. In the future there will be a concept of [plugin registries](https://github.com/dprint/dprint/issues/410) which will allow this to be more distributed.

## Includes and Excludes

The `includes` and `excludes` properties specify the file paths to include and exclude from formatting.

These should be file globs according to [`gitignore`'s extended glob syntax](https://git-scm.com/docs/gitignore#_pattern_format):

```json
{
  // ...omitted...
  "includes": [
    "**/*.{ts,tsx,js,jsx,json}"
  ],
  "excludes": [
    "**/node_modules",
    "**/*-lock.json"
  ]
}
```

## Associations

By default, plugins will pull in files based on their extension. Sometimes a file may have a different extension or no extension at all, but you still want to format it with a certain plugin. The plugin `"associations"` config allows you to do that by associating a certain file pattern to one or multiple plugins.

For example:

```jsonc
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
  "includes": [
    "**/*.*"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

Note that first the `"includes"`/`"excludes"` file resolution occurs and then the associations is used to map those files to a plugin. Specifying associations may also be useful for formatting a file with multiple plugins or forcing a file to be formatted with a specific plugin.

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

Note: The `includes` and `excludes` of extended configuration is ignored for security reasons so you will need to specify them in the main configuration file or via the CLI.

## Incremental

You may specify to only format files that have changed since the last time you formatted the code (recommended):

```jsonc
{
  // etc...
  "incremental": true
  // etc...
}
```

Alternatively, use the `--incremental` flag on the CLI:

```bash
dprint fmt --incremental
```

Doing this will drastically improve performance.

## Global Configuration

There are certain non-language specific configuration that can be specified. These are specified on the main configuration object, but can be overridden on a per-language basis.

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

### `lineWidth`

The width of a line the formatter will try to stay under. Note that this limit will be exceeded in certain cases.

Defaults to `120`.

### `indentWidth`

The number of spaces for an indent when using spaces or the number of characters to treat an indent as when using tabs.

Defaults to `4`.

### `useTabs`

Whether to use tabs (`true`) or spaces (`false`).

Defaults to `false`.

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

Next step: [CLI](/cli)
