# dprint

dprint is a configurable and pluggable code formatting platform.

Currently only TypeScript, JavaScript, and JSONC are supported.

## Future - Moving away from the Node CLI

This project is slowly moving towards a Rust-only CLI independent of any language specific ecosystem.

The configuration, install, and setup instructions below will change. See issue [#194](https://github.com/dprint/dprint/issues/194) for more details.

## Install

Install dprint and the plugins you want to use as a dev dependency.

For example:

```bash
yarn add --dev dprint dprint-plugin-typescript dprint-plugin-jsonc
# or
npm install --save-dev dprint dprint-plugin-typescript dprint-plugin-jsonc
```

## Setup and Usage

Run `npx dprint --init` in the repository's main directory to create a *dprint.config.js* file.

Here's an example:

```js
// @ts-check
const { TypeScriptPlugin } = require("dprint-plugin-typescript");
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    lineWidth: 160,
    plugins: [
        // use this for JS and TS formatting
        new TypeScriptPlugin({
            useBraces: "preferNone",
            "tryStatement.nextControlFlowPosition": "sameLine",
        }),
        new JsoncPlugin({
            indentWidth: 2,
        }),
    ],
    // this could also be specified as a command line argument
    includes: ["**/*.{ts,tsx,js,jsx,json}"],
    // optionally specify file globs for files to ignore
    excludes: [],
};
```

Add a format script to your *package.json*'s "scripts" section (see `npx dprint --help` for usage):

```json
{
  "name": "your-package-name",
  "scripts": {
    "format": "dprint"
  }
}
```

Format:

```bash
yarn format
# or
npm run format
```

## Plugins

* [TypeScript/JavaScript](https://dprint.dev/plugins/typescript)
* [JSON/JSONC](https://dprint.dev/plugins/jsonc)

## Global Configuration

There are certain non-language specific configuration that can be specified. These are specified on the main configuration object, but can be overriden on a per-language basis (with the exception of `projectType`).

For example:

```js
module.exports.config = {
    projectType: "openSource",
    lineWidth: 160,
    useTabs: true,
    plugins: [
        new TypeScriptPlugin({
            lineWidth: 80,
        }),
        new JsoncPlugin({
            indentWidth: 2,
            useTabs: false,
        }),
    ],
};
```

### `projectType`

Specify the type of project dprint is formatting. This is required when using the cli.

You may specify any of the following values according to your conscience:

* `"openSource"` - Dprint is formatting an open source project.
* `"commercialSponsored"` - Dprint is formatting a commercial project and your company sponsored dprint.
* `"commercialDidNotSponsor"` - Dprint is formatting a commercial project and you want to forever enshrine your name in source control for having specified this.

[Sponsoring](https://dprint.dev/sponsor)

### `lineWidth`

The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.

Defaults to `120`.

### `indentWidth`

The number of spaces for an indent when using spaces or the number of characters to treat an indent as when using tabs.

Defaults to `4`.

### `useTabs`

Whether to use tabs (`true`) or spaces (`false`).

Defaults to `false`.
