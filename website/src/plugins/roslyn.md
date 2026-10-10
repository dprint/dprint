---
title: Roslyn Plugin (C#/VB)
description: Documentation on the Roslyn code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Roslyn Plugin

Adapter plugin that formats C# and Visual Basic code via [Roslyn](https://github.com/dotnet/roslyn).

Formats .cs and .vb files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add npm:@dprint/roslyn
# or install from plugins.dprint.dev
dprint add roslyn
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"roslyn"` property to add configuration:

```json
{
  "roslyn": {
    // roslyn's config goes here
  },
  "plugins": [
    "npm:@dprint/roslyn@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/roslyn/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/roslyn)

## Source

See https://github.com/dprint/dprint-plugin-roslyn
