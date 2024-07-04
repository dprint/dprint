---
title: markup_fmt Plugin
description: Documentation on the markup_fmt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/markup_fmt">markup_fmt</a></li>
  </ul>
</nav>

# Markup_fmt Plugin

Adapter plugin that formats HTML, Vue, Svelte, Astro, Jinja, Twig, Nunjucks, and Vento files via [markup_fmt](https://github.com/g-plane/markup_fmt).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add g-plane/markup_fmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"markup"` property (not `"markup_fmt"`) to add configuration:

```jsonc
{
  "markup": { // not "markup_fmt"
    // markup_fmt config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/g-plane/markup_fmt-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/markup_fmt/config)
