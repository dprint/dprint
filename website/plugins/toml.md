---
title: TOML Plugin
description: Documentation on the TOML code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/toml">TOML</a></li>
  </ul>
</nav>

# TOML Code Formatter

Formats [TOML](https://toml.io) files.

## Install and Setup

In a dprint configuration file:

1. Specify the plugin url in the `"plugins"` array.
2. Ensure `.toml` file extensions are matched in an `"includes"` pattern.
3. Add a `"toml"` configuration property if desired.

```jsonc
{
  // omitted...
  "toml": {
    // toml config goes here
  },
  "includes": [
    "**/*.{toml}"
  ],
  "plugins": [
    "https://plugins.dprint.dev/toml-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/toml/config)

## Playground

See [Playground](https://dprint.dev/playground#language/toml)
