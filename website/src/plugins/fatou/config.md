---
title: Configuration - Fatou
description: Documentation on the configuration file for the Fatou code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/fatou">Fatou</a></li>
    <li><a href="/plugins/fatou/config">Configuration</a></li>
  </ul>
</nav>

# Fatou - Configuration

Specify configuration under the `"fatou"` key in your dprint configuration file.

| Property      | Values                                 | Default                           |
| ------------- | -------------------------------------- | --------------------------------- |
| `lineWidth`   | Integer                                | Global `lineWidth`, or `92`       |
| `indentWidth` | Integer                                | Global `indentWidth`, or `4`      |
| `lineEnding`  | `"auto"`, `"lf"`, `"crlf"`, `"native"` | Global `newLineKind`, or `"auto"` |

Fatou always indents with spaces, so the global `useTabs` setting has no effect.

See the [plugin documentation](https://github.com/jolars/dprint-plugin-fatou#configuration) and
[JSON schema](https://plugins.dprint.dev/jolars/fatou/latest/schema.json) for details.
