---
title: WebAssembly Text Plugin
description: Documentation on the WebAssembly Text Format (WAT) code formatting plugin for dprint.
layout: layouts/plugin.njk
---

Wrapper plugin that formats WebAssembly Text Format code via [wat_formatter](https://github.com/g-plane/wasm-language-tools), the formatter of wasm-language-tools. It runs wat_formatter's own code compiled to Wasm, so it formats exactly like the wasm-language-tools language server.

Formats .wat files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add wat
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"wat"` property to add configuration:

```jsonc
{
  "wat": {
    // wat_formatter's config goes here
  },
  "plugins": [
    "npm:@dprint/wat@x.x.x"
  ]
}
```
