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

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add json
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"json"` property to add configuration:

```json
{
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

See [Playground](https://dprint.dev/playground#plugin/json)
