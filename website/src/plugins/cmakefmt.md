---
title: cmakefmt Plugin
description: Documentation on the cmakefmt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/cmakefmt">cmakefmt</a></li>
  </ul>
</nav>

# cmakefmt Plugin

Adapter plugin that formats CMake files via [cmakefmt](https://github.com/cmakefmt/cmakefmt).

Formats CMakeLists.txt, CMakeLists.txt.in, and .cmake files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add sargunv/dprint-cmakefmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"cmakefmt"` property to add configuration:

```jsonc
{
  "cmakefmt": {
    // cmakefmt config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/sargunv/dprint-cmakefmt-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/cmakefmt/config).
