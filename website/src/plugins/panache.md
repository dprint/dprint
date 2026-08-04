---
title: Panache Plugin
description: Documentation on the Panache code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/panache">Panache</a></li>
  </ul>
</nav>

# Panache Plugin

Adapter plugin that formats Quarto, Pandoc, R Markdown, and Markdown files via [Panache](https://panache.bz).

Formats .md, .qmd, .Rmd, and related files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add jolars/panache
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"panache"` property to add configuration:

```json
{
  "panache": {
    // panache config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/panache/config).
