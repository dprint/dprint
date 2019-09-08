# dprint

[![npm version](https://badge.fury.io/js/dprint.svg)](https://badge.fury.io/js/dprint)
[![Build Status](https://travis-ci.org/dsherret/dprint.svg?branch=master)](https://travis-ci.org/dsherret/dprint)

Mono-repo for dprint—a configurable and plugable code formatter.

* [dprint](packages/dprint) - CLI tool
* [@dprint/core](packages/core) - Core functionality
* [dprint-plugin-typescript](packages/dprint-plugin-typescript) - Plugin for TypeScript formatting
* [dprint-plugin-jsonc](packages/dprint-plugin-jsonc) - Plugin for JSONC formatting

## Goals

1. Reasonable configuration.
2. Satisfy my formatting needs.
3. TypeScript and JSONC support.

## Install

Install `dprint` and the plugins you want to use as a dev dependency.

For example:

```
yarn add --dev dprint dprint-plugin-typescript dprint-plugin-jsonc
# or
npm install --save-dev dprint dprint-plugin-typescript dprint-plugin-jsonc
```

## Usage

Create a *dprint.config.js* file in the repo. Here's an example (you don't need to copy this... use your own config):

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
            "tryStatement.nextControlFlowPosition": "sameLine"
        }),
        new JsoncPlugin({
            indentWidth: 2
        })
    ]
};
```

Add a format script to your *package.json*'s "scripts" section (see `npx dprint --help` for usage):

```json
{
  "name": "your-package-name",
  "scripts": {
    "format": "dprint \"**/*{.ts,.tsx,.json,.js}\""
  }
}
```

Format:

```
yarn format
# or
npm run format
```
