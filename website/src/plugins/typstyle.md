---
title: Typstyle Plugin
description: Documentation on the Typstyle code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/typstyle">Typstyle</a></li>
  </ul>
</nav>

# Typstyle Plugin

Adapter plugin that formats Typst files via [Typstyle](https://github.com/typstyle-rs/typstyle).

Formats .typ files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add apcamargo/typstyle
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"typstyle"` property to add configuration:

```json
{
  "typstyle": {
    // typstyle config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/apcamargo/typstyle-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/typstyle/config).
