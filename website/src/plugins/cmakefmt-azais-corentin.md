---
title: cmakefmt (azais-corentin) Plugin
description: Documentation on the cmakefmt (azais-corentin) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# cmakefmt (azais-corentin) Plugin

Adapter plugin that formats CMake files via [azais-corentin/cmakefmt](https://github.com/azais-corentin/cmakefmt).

Formats CMakeLists.txt and .cmake files.

This is a separate plugin from [cmakefmt/cmakefmt](/plugins/cmakefmt). It is configured under the `"cmake"` key.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add azais-corentin/cmakefmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"cmake"` property to add configuration:

```jsonc
{
  "cmake": {
    // cmake config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/azais-corentin/cmakefmt-vx.x.x.wasm"
  ]
}
```

## Configuration

See the [plugin documentation](https://azais-corentin.github.io/cmakefmt/guide/configuration-json.html).
