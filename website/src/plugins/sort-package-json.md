---
title: Sort package.json Plugin
description: Documentation on the package.json sorting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/sort-package-json">Sort package.json</a></li>
  </ul>
</nav>

# Sort package.json Plugin

Formats `package.json` files by sorting keys into a conventional order. It can also sort keys inside the `scripts` object.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add colinaaa/sort-package-json
# or install from npm
dprint add npm:dprint-plugin-sort-package-json
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"sortPackageJson"` property to add configuration:

```json
{
  "sortPackageJson": {
    "sortScripts": true
  },
  "plugins": [
    "https://plugins.dprint.dev/colinaaa/sort-package-json-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/sort-package-json/config).

## Source Code

See [dprint-plugin-sort-package-json](https://github.com/colinaaa/dprint-plugin-sort-package-json).
