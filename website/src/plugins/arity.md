---
title: Arity Plugin
description: Documentation on the Arity code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/arity">Arity</a></li>
  </ul>
</nav>

# Arity Plugin

Adapter plugin that formats R code via [Arity](https://arity.cc).

Formats .R and .r files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add jolars/arity
```

This will update your config file to have an entry for the plugin. Then optionally specify an `"arity"` property to add configuration:

```json
{
  "arity": {
    // arity config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/jolars/arity-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/arity/config).
