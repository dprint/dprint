---
title: TOML Plugin
description: Documentation on the TOML code formatting plugin for dprint.
layout: layouts/documentation.njk
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

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add toml
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"toml"` property to add configuration:

```json
{
  // omitted...
  "toml": {
    // toml config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/toml-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/toml/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/toml)
