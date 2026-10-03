---
title: Configuration - Lax SQL
description: Documentation on the configuration file for the Lax SQL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-sql">Lax SQL</a></li>
    <li><a href="/plugins/lax-sql/config">Configuration</a></li>
  </ul>
</nav>

# Lax SQL - Configuration

Specify configuration under the `"sql"` key in your dprint configuration file.

| Property                | Values                                 | Default                         |
| ----------------------- | -------------------------------------- | ------------------------------- |
| `lineWidth`             | Integer                                | Global `lineWidth`, or `120`    |
| `indentWidth`           | Integer                                | Global `indentWidth`, or `2`    |
| `useTabs`               | Boolean                                | Global `useTabs`, or `false`    |
| `newLineKind`           | `"auto"`, `"lf"`, `"crlf"`, `"system"` | Global `newLineKind`, or `"lf"` |
| `keywordCase`           | `"preserve"`, `"upper"`, `"lower"`     | `"preserve"`                    |
| `clauseStyle`           | `"fill"`, `"expanded"`                 | `"fill"`                        |
| `ignoreNodeCommentText` | String                                 | `"dprint-ignore"`               |
| `ignoreFileCommentText` | String                                 | `"dprint-ignore-file"`          |

## Clause Style

`clauseStyle` controls how a clause body is laid out:

- `"fill"` - The clause body flows after the keyword and wraps at the line width, packing items until they no longer fit.
- `"expanded"` - The clause keyword sits alone and the body is indented below it, with one comma separated item per line.

## Keyword Casing

`keywordCase` is the one option that changes tokens. With `"upper"` or `"lower"`, only words on a curated list of SQL keywords are changed; quoted identifiers and function names are never affected.

See the [plugin documentation](https://github.com/bartlomieju/lax/tree/main/crates/lax-sql#configuration) for details.
