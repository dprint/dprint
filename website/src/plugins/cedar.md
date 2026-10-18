---
title: Cedar Plugin
description: Documentation on the Cedar code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Cedar Plugin

Adapter plugin that formats [Cedar](https://www.cedarpolicy.com) policy files via [cedar-policy-formatter](https://github.com/cedar-policy/cedar).

Formats .cedar files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add cedar
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"cedar"` property to add configuration:

```json
{
  "cedar": {
    // cedar's config goes here
  },
  "plugins": [
    "npm:@dprint/cedar@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/cedar/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/cedar)

## Source

See https://github.com/dprint/dprint-plugin-cedar
