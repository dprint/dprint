---
title: shfmt Plugin
description: Documentation on the shfmt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/shfmt">shfmt</a></li>
  </ul>
</nav>

# shfmt Plugin

Adapter plugin that formats shell script code via [shfmt](https://github.com/mvdan/sh).

Formats .sh, .bash, .zsh, .mksh, and .bats files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add hrko/shfmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"shfmt"` property to add configuration:

```json
{
  "shfmt": {
    // shfmt's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/hrko/shfmt-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/shfmt/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/shfmt)

## Source

See https://github.com/hrko/dprint-plugin-shfmt
