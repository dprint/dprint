---
title: Typstyle Plugin
description: Documentation on the Typstyle code formatting plugin for dprint.
layout: layouts/documentation.njk
oldUrl: /plugins/typstyle-kachick/
---

# Typstyle Plugin

Adapter plugin that formats [Typst](https://typst.app) files via [Typstyle](https://github.com/typstyle-rs/typstyle).

Formats .typ files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add typstyle
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"typst"` property to add configuration:

```jsonc
{
  "typst": {
    // typstyle's config goes here
  },
  "plugins": [
    "npm:@dprint/typstyle@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/typstyle/config).

## Source

See https://github.com/dprint/dprint-plugin-typstyle
