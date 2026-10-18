---
title: TOML Plugin
description: Documentation on the TOML code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# TOML Code Formatter

Formats [TOML](https://toml.io) files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add toml
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"toml"` property to add configuration:

```json
{
  "toml": {
    // toml config goes here
  },
  "plugins": [
    "npm:@dprint/toml@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/toml/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/toml)
