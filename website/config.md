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
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "commercialTrial",
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
    "**/*.{ts,tsx,js,jsx,json}"
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

## `$schema`

This property is optional and provides auto-completion support in Visual Studio Code.

## Project Type

The `"projectType"` specifies the type of license being used to format the project.

You must specify any of the following values:

- `"openSource"` - Dprint is formatting an open source project whose primary maintainer is not a for-profit company (free).
- `"educational"` - Dprint is formatting a project run by a student or being used for educational purposes (free).
- `"nonProfit"` - Dprint is formatting a project whose primary maintainer is a non-profit organization (free).
- `"commercialPaid"` - Dprint is formatting a project whose primary maintainer is a for-profit company or individual and the primary maintainer paid for a commercial license. Thank you for being part of moving this project forward!
- `"commercialTrial"` - Dprint is formatting a project whose primary maintainer is a for-profit company or individual and it is being evaluated for 30 days.

See [Pricing](/pricing) for more details.

## Plugins

The `plugins` property specifies which plugins to use for formatting. These may be URLs or file paths to a web assembly file of the plugin.

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

You may specify to only format files that have changed since the last time you ran `dprint fmt` or `dprint check` by specifying `"incremental": true`:

```jsonc
{
  // etc...
  "incremental": true
  // etc...
}
```

Alternatively, you may specify an `--incremental` flag on the CLI:

```bash
dprint fmt --incremental
```

Doing this will drastically improve performance.

## Global Configuration

There are certain non-language specific configuration that can be specified. These are specified on the main configuration object, but can be overridden on a per-language basis.

For example:

```json
{
  "projectType": "openSource",
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
  "$schema": "https://dprint.dev/schemas/v0.json",
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
  "$schema": "https://dprint.dev/schemas/v0.json",
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
  "$schema": "https://dprint.dev/schemas/v0.json",
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
