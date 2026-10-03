---
title: KDL Plugin
description: Documentation on the KDL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/kdl">KDL</a></li>
  </ul>
</nav>

# KDL Plugin

Formats [KDL](https://kdl.dev) documents.

Formats .kdl files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add kachick/kdl
# or install from npm
dprint add npm:@kachick/dprint-plugin-kdl
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"kdl"` property to add configuration:

```json
{
  "kdl": {
    // kdl config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/kachick/kdl-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/kdl/config).
