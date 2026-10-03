---
title: Lax Markup Plugin
description: Documentation on the Lax Markup code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-markup">Lax Markup</a></li>
  </ul>
</nav>

# Lax Markup Plugin

Formats HTML, XML, SVG, Vue, Svelte, and Astro files via [lax-markup](https://github.com/bartlomieju/lax/tree/main/crates/lax-markup), a formatter that only restructures markup where doing so cannot change how it renders.

Formats .html, .htm, .vue, .svelte, .astro, .xml, and .svg files.

This plugin uses the same `"markup"` configuration key as [markup_fmt](/plugins/markup_fmt).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add bartlomieju/lax-markup
# or install from npm
dprint add npm:lax-markup
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"markup"` property to add configuration:

```json
{
  "markup": {
    // markup config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/bartlomieju/lax-markup-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/lax-markup/config).
