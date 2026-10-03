---
title: Configuration - Typstyle (kachick)
description: Documentation on the configuration file for the Typstyle (kachick) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/typstyle-kachick">Typstyle (kachick)</a></li>
    <li><a href="/plugins/typstyle-kachick/config">Configuration</a></li>
  </ul>
</nav>

# Typstyle (kachick) - Configuration

Specify configuration under the `"typst"` key in your dprint configuration file.

| Property               | Values                           | Default                      |
| ---------------------- | -------------------------------- | ---------------------------- |
| `lineWidth`            | Integer                          | Global `lineWidth`, or `80`  |
| `indentWidth`          | Integer                          | Global `indentWidth`, or `2` |
| `blankLinesUpperBound` | Integer                          | `1`                          |
| `reorderImportItems`   | Boolean                          | `true`                       |
| `wrapMode`             | `"none"`, `"fill"`, `"sentence"` | `"none"`                     |

See the [plugin documentation](https://github.com/kachick/dprint-plugin-typstyle#configuration-example) and
[JSON schema](https://plugins.dprint.dev/kachick/typstyle/latest/schema.json) for details.
