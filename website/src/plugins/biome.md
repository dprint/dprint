---
title: Biome Plugin
description: Documentation on the Biome code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/biome">Biome</a></li>
  </ul>
</nav>

# Biome Plugin

Adapter plugin that formats JavaScript, TypeScript, and JSON files via [Biome](https://biomejs.dev).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add biome
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"biome"` property to add configuration:

```json
{
  "biome": {
    // biome's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/biome-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/biome/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/biome)
