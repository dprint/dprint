---
title: YAML Plugin
description: Documentation on the YAML code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/yaml">YAML</a></li>
  </ul>
</nav>

# YAML Plugin

Adapter plugin that formats YAML files via [Pretty YAML](https://github.com/g-plane/pretty_yaml).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add g-plane/pretty_yaml
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"yaml"` property to add configuration:

```json
{
  "yaml": { // not "pretty_yaml"
    // Pretty YAML config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/g-plane/pretty_yaml-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/yaml/config)
