---
title: TypeScript / JavaScript Plugin
description: Documentation on the TypeScript / JavaScript code formatting plugin for dprint.
layout: layouts/plugin.njk
---

Supports:

- TypeScript
- JavaScript - Supports all the JS syntax that the TS compiler supports.
- JSX/TSX

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add typescript
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"typescript"` property to add configuration:

```json
{
  // ...etc...
  "typescript": {
    // TypeScript & JavaScript config goes here
  },
  "plugins": [
    // ...etc...
    "npm:@dprint/typescript@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/typescript/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/typescript)

## Ignoring Files

Add an ignore file comment as the **first** comment in the file:

```ts
// dprint-ignore-file
```

## Ignore Comments

Add an ignore comment before the code:

```ts
// dprint-ignore
const identity = [
    1, 0, 0,
    0, 1, 0,
    0, 0, 1,
];

// or even...

const identity = /* dprint-ignore */ [
    1, 0, 0,
    0, 1, 0,
    0, 0, 1,
];
```
