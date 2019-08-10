# dprint

[![npm version](https://badge.fury.io/js/dprint.svg)](https://badge.fury.io/js/dprint)
[![Build Status](https://travis-ci.org/dsherret/dprint.svg?branch=master)](https://travis-ci.org/dsherret/dprint)

Mono-repo for dprint—a plugable and configurable code formatter.

* [dprint](packages/dprint)
* [@dprint/core](packages/core)
* [dprint-plugin-typescript](packages/dprint-plugin-typescript)
* [dprint-plugin-jsonc](packages/dprint-plugin-jsonc)

## Usage

Install dprint and the plugins you want to use as a dev dependency.

For example:

```
yarn add --dev dprint dprint-plugin-typescript dprint-plugin-jsonc
# or
npm install --save-dev dprint dprint-plugin-typescript dprint-plugin-jsonc
```

Create a *dprint.json* file in the repo. Here's an example (you don't need to copy this... use your own config):

```json
{
  "projectType": "openSource",
  "lineWidth": 160,
  "jsonc": {
    "indentWidth": 2
  },
  "typescript": {
    "tryStatement.nextControlFlowPosition": "sameLine"
  },
  "plugins": [
    "dprint-plugin-typescript",
    "dprint-plugin-jsonc"
  ]
}
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
