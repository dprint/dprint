---
title: clang-format Plugin (C/C++/Objective-C)
description: Documentation on the clang-format code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# clang-format Plugin

Wrapper plugin that formats C, C++, and Objective-C code via a Wasm build of [clang-format](https://clang.llvm.org/docs/ClangFormat.html).

Formats .c, .cc, .cpp, .cxx, .h, .hh, .hpp, .hxx, .m, and .mm files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add clang-format
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"clangFormat"` property to add configuration:

```json
{
  "clangFormat": {
    // clang-format's config goes here
  },
  "plugins": [
    "npm:@dprint/clang-format@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/clang-format/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/clang-format)

## Source

See https://github.com/dprint/dprint-plugin-clang-format
