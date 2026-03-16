---
title: Gofumpt Plugin
description: Documentation on the gofumpt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/gofumpt">Gofumpt</a></li>
  </ul>
</nav>

# Gofumpt Plugin

Adapter plugin that formats Go code via [gofumpt](https://github.com/mvdan/gofumpt).

Formats .go files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add jakebailey/gofumpt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"gofumpt"` property to add configuration:

```json
{
  "gofumpt": {
    // gofumpt config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/jakebailey/gofumpt-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/gofumpt/config)
