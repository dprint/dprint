---
title: StyLua Plugin
description: Documentation on the StyLua (Lua and Luau) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# StyLua Plugin

Wrapper plugin that formats Lua and Luau code via [StyLua](https://github.com/JohnnyMorganz/StyLua). It runs StyLua's own code compiled to Wasm, so it formats exactly like the StyLua CLI.

Formats .lua and .luau files. Lua 5.1 through 5.4, Luau, LuaJIT, and CfxLua are supported.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add stylua
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"stylua"` property to add configuration:

```jsonc
{
  "stylua": {
    // StyLua's config goes here
  },
  "plugins": [
    "npm:@dprint/stylua@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/stylua/config).

## Source

See https://github.com/dprint/dprint-plugin-stylua
