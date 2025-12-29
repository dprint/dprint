---
title: Oxc Plugin
description: Documentation on the Oxc code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/oxc">Oxc</a></li>
  </ul>
</nav>

# Oxc Plugin

Adapter plugin that formats JavaScript and TypeScript files via [Oxc](https://oxc.rs).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add oxc
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"oxc"` property to add configuration:

```json
{
  "oxc": {
    // oxc's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/oxc-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/oxc/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/oxc)

## Source

See https://github.com/dprint/dprint-plugin-oxc
