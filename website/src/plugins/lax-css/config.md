---
title: Configuration - Lax CSS
description: Documentation on the configuration file for the Lax CSS code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-css">Lax CSS</a></li>
    <li><a href="/plugins/lax-css/config">Configuration</a></li>
  </ul>
</nav>

# Lax CSS - Configuration

Specify configuration under the `"css"` key in your dprint configuration file.

| Property                | Values                                 | Default                         |
| ----------------------- | -------------------------------------- | ------------------------------- |
| `lineWidth`             | Integer                                | Global `lineWidth`, or `120`    |
| `indentWidth`           | Integer                                | Global `indentWidth`, or `2`    |
| `useTabs`               | Boolean                                | Global `useTabs`, or `false`    |
| `newLineKind`           | `"auto"`, `"lf"`, `"crlf"`, `"system"` | Global `newLineKind`, or `"lf"` |
| `ignoreNodeCommentText` | String                                 | `"dprint-ignore"`               |
| `ignoreFileCommentText` | String                                 | `"dprint-ignore-file"`          |

Long values and at-rule preludes wrap at `lineWidth`, but a line break is only introduced where there was already whitespace. Selectors are never wrapped.

See the [plugin documentation](https://github.com/bartlomieju/lax/tree/main/crates/lax-css#configuration) for details.
