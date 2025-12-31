---
title: Mago Plugin
description: Documentation on the Mago code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/mago">Mago</a></li>
  </ul>
</nav>

# Mago Plugin

Adapter plugin that formats PHP code via [Mago](https://github.com/carthage-software/mago).

Formats .php files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add mago
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"mago"` property to add configuration:

```json
{
  "mago": {
    // mago's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/mago-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/mago/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/mago)

## Source

See https://github.com/dprint/dprint-plugin-mago
