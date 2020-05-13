# dprint

[![npm version](https://badge.fury.io/js/dprint.svg)](https://badge.fury.io/js/dprint)
[![Build Status](https://travis-ci.com/dprint/dprint.svg?branch=master)](https://travis-ci.com/dprint/dprint)

Mono-repo for dprint—a configurable and pluggable code formatter.

This project is under active early development. I recommend you check its output to ensure it's doing its job correctly and only run this on code that has been checked into source control.

Currently only TypeScript, Javascript, and JSONC are supported.

## Links

* [Overview](https://dprint.dev)
* [Playground](https://dprint.dev/playground)
* [Algorithm overview](docs/overview.md)

## Future - Moving away from the Node CLI

This project is slowly moving towards a Rust-only CLI independent of any language specific ecosystem.

The configuration, install, and setup instructions below will change. See issue [#194](https://github.com/dprint/dprint/issues/194) for more details.

## Install

Install `dprint` and the plugins you want to use as a dev dependency.

For example:

```
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
        new TypeScriptPlugin({
            useBraces: "preferNone",
            "tryStatement.nextControlFlowPosition": "sameLine",
        }),
        new JsoncPlugin({
            indentWidth: 2,
        }),
    ],
    // this could also be specified as a command line argument
    includes: ["**/*.{ts,tsx,json,js,jsx}"],
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

```
yarn format
# or
npm run format
```
