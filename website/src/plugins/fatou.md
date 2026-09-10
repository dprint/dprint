---
title: Fatou Plugin
description: Documentation on the Fatou code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/fatou">Fatou</a></li>
  </ul>
</nav>

# Fatou Plugin

Adapter plugin that formats Julia code via [Fatou](https://fatou.dev).

Formats .jl files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add jolars/fatou
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"fatou"` property to add configuration:

```json
{
  "fatou": {
    // fatou config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/jolars/fatou-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/fatou/config).
