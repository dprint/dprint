---
title: clang-format Plugin
description: Documentation on the clang-format code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/clang-format">clang-format</a></li>
  </ul>
</nav>

# clang-format Plugin

Formats C, C++, and Objective-C code via a Wasm port of [clang-format](https://clang.llvm.org/docs/ClangFormat.html).

Formats .c, .cc, .cpp, .cxx, .h, .hh, .hpp, .hxx, .m, and .mm files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add sargunv/dprint-clang-format
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"clangFormat"` property to add configuration:

```json
{
  "clangFormat": {
    // clangFormat config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/sargunv/dprint-clang-format-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/clang-format/config).
