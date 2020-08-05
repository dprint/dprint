---
title: Prettier Plugin
description: Documentation on the Prettier code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/prettier">Prettier</a></li>
  </ul>
</nav>

# Prettier Plugin

Wrapper plugin that formats [many languages](https://prettier.io/docs/en/index.html) via [Prettier](https://prettier.io).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike WASM plugins).
  </div>
</div>

## Install and Setup

In _.dprintrc.json_:

1. Specify the plugin url in the `"plugins"` array (follow instructions at [https://github.com/dprint/dprint-plugin-prettier/releases/](https://github.com/dprint/dprint-plugin-prettier/releases/)).
2. Ensure the file extensions supported by prettier are matched in an `"includes"` pattern.
3. Add a `"prettier"` configuration property if desired.

```jsonc
{
  // ...etc...
  "prettier": {
    "trailingComma": "all",
    "singleQuote": true,
    "proseWrap": "always"
  }
}
```

## Configuration

See Prettier's configuration [here](https://prettier.io/docs/en/options.html). Specify using the "API Override" column.
