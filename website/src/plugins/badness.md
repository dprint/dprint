---
title: Badness Plugin
description: Documentation on the Badness code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/badness">Badness</a></li>
  </ul>
</nav>

# Badness Plugin

Adapter plugin that formats LaTeX and BibTeX files via [Badness](https://badness.dev).

Formats .tex, .sty, .cls, .dtx, .ins, and .bib files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add jolars/badness
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"badness"` property to add configuration:

```json
{
  "badness": {
    // badness config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/jolars/badness-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/badness/config).

Unlike the Badness CLI, the Wasm plugin cannot read command signatures from neighboring .sty and .cls files. See the
[plugin documentation](https://github.com/jolars/dprint-plugin-badness#differences-from-the-badness-cli) for details.
