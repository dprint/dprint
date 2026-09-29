---
title: bibtex-tidy Plugin
description: Documentation on the bibtex-tidy code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/bibtex-tidy">bibtex-tidy</a></li>
  </ul>
</nav>

# bibtex-tidy Plugin

Adapter plugin that formats BibTeX files via [bibtex-tidy](https://github.com/FlamingTempura/bibtex-tidy).

Formats .bib and .bibtex files. `dprint add` currently seeds includes for `.bib` only; add `**/*.bibtex` yourself if you use that extension.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add apcamargo/bibtex-tidy
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"bibtex-tidy"` property to add configuration:

```json
{
  "bibtex-tidy": {
    // bibtex-tidy config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/apcamargo/bibtex-tidy-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/bibtex-tidy/config).
