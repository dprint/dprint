---
title: Typstyle (kachick) Plugin
description: Documentation on the Typstyle (kachick) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/typstyle-kachick">Typstyle (kachick)</a></li>
  </ul>
</nav>

# Typstyle (kachick) Plugin

Adapter plugin that formats Typst files via [Typstyle](https://github.com/typstyle-rs/typstyle).

Formats .typ files.

This is a separate plugin from [apcamargo/typstyle](/plugins/typstyle). It is configured under the `"typst"` key and is also published to npm.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add kachick/typstyle
# or install from npm
dprint add npm:@kachick/dprint-plugin-typstyle
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"typst"` property to add configuration:

```jsonc
{
  "typst": {
    // typst config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/kachick/typstyle-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/typstyle-kachick/config).
