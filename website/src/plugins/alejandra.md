---
title: Alejandra Plugin
description: Documentation on the Alejandra (Nix) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Alejandra Plugin

Wrapper plugin that formats Nix code via [Alejandra](https://github.com/kamadorueda/alejandra). It runs Alejandra's own code compiled to Wasm, so it formats like the Alejandra CLI.

Formats .nix files. It matches the same files as the [Nix plugin](/plugins/nix), so use one or the other.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add alejandra
```

This will update your config file to have an entry for the plugin. Then optionally specify an `"alejandra"` property to add configuration:

```jsonc
{
  "alejandra": {
    // Alejandra's config goes here
  },
  "plugins": [
    "npm:@dprint/alejandra@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/alejandra/config).

## Source

See https://github.com/dprint/dprint-plugin-alejandra
