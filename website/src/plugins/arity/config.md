---
title: Configuration - Arity
description: Documentation on the configuration file for the Arity code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/arity">Arity</a></li>
    <li><a href="/plugins/arity/config">Configuration</a></li>
  </ul>
</nav>

# Arity - Configuration

Specify configuration under the `"arity"` key in your dprint configuration file.

| Property          | Values                                 | Default                           |
| ----------------- | -------------------------------------- | --------------------------------- |
| `lineWidth`       | Integer                                | Global `lineWidth`, or `80`       |
| `indentWidth`     | Integer                                | Global `indentWidth`, or `2`      |
| `lineEnding`      | `"auto"`, `"lf"`, `"crlf"`, `"native"` | Global `newLineKind`, or `"auto"` |
| `roxygenMarkdown` | Boolean                                | `false`                           |

Arity always indents with spaces, so the global `useTabs` setting has no effect.

## Roxygen Markdown

Set `roxygenMarkdown` to `true` when your package enables Markdown throughout its roxygen comments with `Roxygen: list(markdown = TRUE)` in DESCRIPTION.
The Wasm plugin cannot read DESCRIPTION or man/roxygen/meta.R to discover this setting, so it must be configured explicitly:

```json
{
  "arity": {
    "roxygenMarkdown": true
  }
}
```

Per-block `@md` and `@noMd` tags override this default.

See the [plugin documentation](https://github.com/jolars/dprint-plugin-arity#configuration) and
[JSON schema](https://plugins.dprint.dev/jolars/arity/latest/schema.json) for details.
