---
title: Dockerfile Plugin
description: Documentation on the Dockerfile code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Dockerfile Code Formatter

Formats [Dockerfiles](https://docs.docker.com/engine/reference/builder).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add dockerfile
# or install from npm
dprint add npm:@dprint/dockerfile
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"dockerfile"` property to add configuration:

```json
{
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
