---
title: Ruff Plugin
description: Documentation on the Ruff code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/ruff">Ruff</a></li>
  </ul>
</nav>

# Ruff Plugin

Adapter plugin that formats Python code via [Ruff](https://docs.astral.sh/ruff/).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add ruff
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"ruff"` property to add configuration:

```json
{
  "ruff": {
    // ruff's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/ruff-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/ruff/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/ruff)
