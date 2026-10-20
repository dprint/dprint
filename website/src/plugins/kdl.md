---
title: KDL Plugin
description: Documentation on the KDL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# KDL Plugin

Formats [KDL](https://kdl.dev) documents.

Formats .kdl files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add kdl
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"kdl"` property to add configuration:

```jsonc
{
  "kdl": {
    // kdl config goes here
  },
  "plugins": [
    "npm:@dprint/kdl@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/kdl/config).

## Source

See https://github.com/dprint/dprint-plugin-kdl
