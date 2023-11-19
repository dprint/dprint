---
title: Dockerfile Plugin
description: Documentation on the Dockerfile code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/dockerfile">Dockerfile</a></li>
  </ul>
</nav>

# Dockerfile Code Formatter

Formats [Dockerfiles](https://docs.docker.com/engine/reference/builder).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add dockerfile
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"dockerfile"` property to add configuration:

```json
{
  // omitted...
  "dockerfile": {
    // dockerfile config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/dockerfile-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/dockerfile/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/dockerfile)
