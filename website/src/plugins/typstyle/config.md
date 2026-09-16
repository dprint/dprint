---
title: Configuration - Typstyle
description: Documentation on the configuration file for the Typstyle code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/typstyle">Typstyle</a></li>
    <li><a href="/plugins/typstyle/config">Configuration</a></li>
  </ul>
</nav>

# Typstyle - Configuration

Specify configuration under the `"typstyle"` key in your dprint configuration file.

| Property               | Values                           | Default                                      |
| ---------------------- | -------------------------------- | -------------------------------------------- |
| `lineWidth`            | Integer                          | Global `lineWidth`, or `80`                  |
| `indentWidth`          | Integer                          | Global `indentWidth`, or `2`                 |
| `blankLinesUpperBound` | Integer                          | `1`                                          |
| `collapseMarkupSpaces` | Boolean                          | `false`                                      |
| `reorderImportItems`   | Boolean                          | `true`                                       |
| `wrapMode`             | `"none"`, `"fill"`, `"sentence"` | `"none"`                                     |
| `lineEnding`           | `"lf"`, `"crlf"`                 | Global `newLineKind` (CRLF or LF), or `"lf"` |

Typstyle always indents with spaces, so the global `useTabs` setting has no effect.
A global `newLineKind` of `"auto"` or `"system"` is treated as `"lf"`.

See the [plugin documentation](https://github.com/apcamargo/dprint-plugin-typstyle#configuration) and
[JSON schema](https://plugins.dprint.dev/apcamargo/typstyle/latest/schema.json) for details.
