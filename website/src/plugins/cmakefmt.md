---
title: cmakefmt Plugin
description: Documentation on the cmakefmt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# cmakefmt Plugin

Plugin that formats CMake files via [cmakefmt](https://github.com/cmakefmt/cmakefmt).

Formats CMakeLists.txt, CMakeLists.txt.in, and .cmake files.

There is also a separate [azais-corentin/cmakefmt](/plugins/cmakefmt-azais-corentin) plugin, which is configured under the `"cmake"` key.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add cmakefmt/cmakefmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"cmakefmt"` property to add configuration:

```jsonc
{
  "cmakefmt": {
    // cmakefmt config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/cmakefmt/cmakefmt-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/cmakefmt/config).
