---
title: Lax CSS Plugin
description: Documentation on the Lax CSS code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-css">Lax CSS</a></li>
  </ul>
</nav>

# Lax CSS Plugin

Formats CSS, SCSS, and Less files via [lax-css](https://github.com/bartlomieju/lax/tree/main/crates/lax-css), a formatter that only adjusts whitespace, indentation, and line breaks and never rewrites your styles.

Formats .css, .scss, and .less files. The indented Sass syntax (.sass) is not supported; use [Malva](/plugins/malva) for that.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add bartlomieju/lax-css
# or install from npm
dprint add npm:lax-css
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"css"` property to add configuration:

```json
{
  "css": {
    // css config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/bartlomieju/lax-css-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/lax-css/config).
