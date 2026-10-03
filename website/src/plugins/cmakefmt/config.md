---
title: Configuration - cmakefmt
description: Documentation on the configuration file for the cmakefmt code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/cmakefmt">cmakefmt</a></li>
    <li><a href="/plugins/cmakefmt/config">Configuration</a></li>
  </ul>
</nav>

# cmakefmt - Configuration

Specify configuration under the `"cmakefmt"` key in your dprint configuration file. Options without a listed default use cmakefmt's default.

| Property                       | Values                                 | Default              |
| ------------------------------ | -------------------------------------- | -------------------- |
| `lineWidth`                    | Integer                                | Global `lineWidth`   |
| `indentWidth`                  | Integer                                | Global `indentWidth` |
| `useTabs`                      | Boolean                                | Global `useTabs`     |
| `newLineKind`                  | `"auto"`, `"lf"`, `"crlf"`             | Global `newLineKind` |
| `commandCase`                  | `"lower"`, `"upper"`, `"unchanged"`    |                      |
| `keywordCase`                  | `"lower"`, `"upper"`, `"unchanged"`    |                      |
| `maxEmptyLines`                | Integer                                |                      |
| `maxLinesHwrap`                | Integer                                |                      |
| `maxHangingWrapPositionalArgs` | Integer                                |                      |
| `maxHangingWrapGroups`         | Integer                                |                      |
| `maxRowsCmdline`               | Integer                                |                      |
| `requireValidLayout`           | Boolean                                |                      |
| `wrapAfterFirstArg`            | Boolean                                |                      |
| `continuationAlign`            | `"same-indent"`, `"under-first-value"` |                      |
| `enableSort`                   | Boolean                                |                      |
| `autosort`                     | Boolean                                |                      |
| `dangleParens`                 | Boolean                                |                      |
| `dangleAlign`                  | `"prefix"`, `"open"`, `"close"`        |                      |
| `enableMarkup`                 | Boolean                                |                      |
| `firstCommentIsLiteral`        | Boolean                                |                      |

## Limitations

- The plugin does not read cmakefmt config files (`.cmakefmt.yaml`, `.cmakefmt.yml`, or `.cmakefmt.toml`). Put formatter options in your dprint configuration instead.
- Range formatting is not supported.

See the [plugin documentation](https://github.com/sargunv/dprint-cmakefmt#configure) and
[JSON schema](https://plugins.dprint.dev/sargunv/dprint-cmakefmt/latest/schema.json) for details.
