---
title: JSON Plugin
description: Documentation on the JSON code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/json">JSON</a></li>
  </ul>
</nav>

# JSON/JSONC Code Formatter

Supports:

- JSON
- JSONC (JSON with comments)

## Install and Setup

In a dprint configuration file:

1. Specify the plugin url in the `"plugins"` array.
2. Add a `"json"` configuration property if desired.

```json
{
  // omitted...
  "json": {
    // json config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/json/config)

## Playground

See [Playground](https://dprint.dev/playground#language/json)
