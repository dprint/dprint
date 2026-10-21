---
title: Zig Plugin
description: Documentation on the Zig code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Zig Plugin

Wrapper plugin that formats Zig code via `zig fmt`. It runs the formatter from Zig's standard library compiled to Wasm, so it formats exactly like `zig fmt`.

Formats .zig and .zon files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add zig
```

This will update your config file to have an entry for the plugin.

```json
{
  "plugins": [
    "npm:@dprint/zig@x.x.x"
  ]
}
```

## Configuration

There's nothing to configure, since `zig fmt` has no formatting options. The global configuration doesn't apply either, so files are always indented with four spaces and have LF newlines, like with `zig fmt`.

## Source

See https://github.com/dprint/dprint-plugin-zig
