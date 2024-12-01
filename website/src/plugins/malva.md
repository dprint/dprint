---
title: Malva Plugin
description: Documentation on the Malva code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/malva">Malva</a></li>
  </ul>
</nav>

# Malva Plugin

Adapter plugin that formats CSS, SCSS, Sass (indented syntax), and Less files via [Malva](https://github.com/g-plane/malva).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add g-plane/malva
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"malva"` property to add configuration:

```json
{
  "malva": {
    // malva config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/g-plane/malva-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/malva/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/malva)
