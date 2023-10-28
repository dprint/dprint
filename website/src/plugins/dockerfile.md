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

In a dprint configuration file:

1. Specify the plugin url in the `"plugins"` array.
2. Add a `"dockerfile"` configuration property if desired.

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

See [Playground](https://dprint.dev/playground#language/dockerfile)
