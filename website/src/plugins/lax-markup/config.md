---
title: Configuration - Lax Markup
description: Documentation on the configuration file for the Lax Markup code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-markup">Lax Markup</a></li>
    <li><a href="/plugins/lax-markup/config">Configuration</a></li>
  </ul>
</nav>

# Lax Markup - Configuration

Specify configuration under the `"markup"` key in your dprint configuration file.

| Property                | Values                                 | Default                         |
| ----------------------- | -------------------------------------- | ------------------------------- |
| `lineWidth`             | Integer                                | Global `lineWidth`, or `120`    |
| `indentWidth`           | Integer                                | Global `indentWidth`, or `2`    |
| `useTabs`               | Boolean                                | Global `useTabs`, or `false`    |
| `newLineKind`           | `"auto"`, `"lf"`, `"crlf"`, `"system"` | Global `newLineKind`, or `"lf"` |
| `ignoreNodeCommentText` | String                                 | `"dprint-ignore"`               |
| `ignoreFileCommentText` | String                                 | `"dprint-ignore-file"`          |

`<!-- dprint-ignore -->` and `<!-- dprint-ignore-file -->` comments are supported, and their text is configurable with `ignoreNodeCommentText` and `ignoreFileCommentText`.

See the [plugin documentation](https://github.com/bartlomieju/lax/tree/main/crates/lax-markup#configuration) for details.
