---
title: buildifier Plugin
description: Documentation on the buildifier (Bazel/Starlark) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# buildifier Plugin

Wrapper plugin that formats Bazel and Starlark files via [buildifier](https://github.com/bazel-contrib/buildtools/tree/main/buildifier). It runs buildifier's own code compiled to Wasm, so it formats like the buildifier CLI.

Formats `BUILD`, `BUILD.bazel`, `MODULE.bazel`, `WORKSPACE`, `WORKSPACE.bazel`, `WORKSPACE.bzlmod`, `REPO.bazel` and `VENDOR.bazel` files as Bazel files, and .bzl, .star and .sky files as plain Starlark. Like buildifier, it picks how to format a file from its name.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add buildifier
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"buildifier"` property to add configuration:

```jsonc
{
  "buildifier": {
    // buildifier's config goes here
  },
  "plugins": [
    "npm:@dprint/buildifier@x.x.x"
  ]
}
```

## Configuration

buildifier has no style options, so the only option is `newLineKind`. See [Configuration](/plugins/buildifier/config).

## Source

See https://github.com/dprint/dprint-plugin-buildifier
