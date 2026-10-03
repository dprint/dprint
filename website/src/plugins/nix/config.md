---
title: Configuration - Nix
description: Documentation on the configuration file for the Nix code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/nix">Nix</a></li>
    <li><a href="/plugins/nix/config">Configuration</a></li>
  </ul>
</nav>

# Nix - Configuration

Specify configuration under the `"nix"` key in your dprint configuration file.

| Property      | Values  | Default                      |
| ------------- | ------- | ---------------------------- |
| `lineWidth`   | Integer | Global `lineWidth`, or `100` |
| `indentWidth` | Integer | Global `indentWidth`, or `2` |

See the [plugin documentation](https://github.com/kachick/dprint-plugin-nix#configuration) and
[JSON schema](https://plugins.dprint.dev/kachick/nix/latest/schema.json) for details.
