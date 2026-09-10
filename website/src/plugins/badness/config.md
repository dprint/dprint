---
title: Configuration - Badness
description: Documentation on the configuration file for the Badness code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/badness">Badness</a></li>
    <li><a href="/plugins/badness/config">Configuration</a></li>
  </ul>
</nav>

# Badness - Configuration

Specify configuration under the `"badness"` key in your dprint configuration file.

| Property               | Values                                                            | Default                           |
| ---------------------- | ----------------------------------------------------------------- | --------------------------------- |
| `lineWidth`            | Integer from 1 to 1000                                            | Global `lineWidth`, or `80`       |
| `indentWidth`          | Integer from 1 to 1000                                            | Global `indentWidth`, or `2`      |
| `itemIndent`           | `"hang"`, `"indent"`, `"none"`                                    | `"hang"`                          |
| `wrap`                 | `"reflow"`, `"stable"`, `"sentence"`, `"semantic"`, `"preserve"`  | Depends on file kind (see below)  |
| `mathWrap`             | `"auto"`, `"preserve"`, `"single-line"`, `"break"`                | `"auto"`                          |
| `lineEnding`           | `"auto"`, `"lf"`, `"crlf"`, `"native"`                            | Global `newLineKind`, or `"auto"` |
| `lang`                 | Language code, such as `"en"`, `"de"`, or `"pt-BR"`               | Unset (English)                   |
| `noBreakAbbreviations` | Object mapping language codes or `"default"` to arrays of strings | `{}`                              |

When `wrap` is unset, .tex files use `"reflow"`, while .sty, .cls, .dtx, .ins, and *.code.tex files preserve authored line breaks.
The `lang` and `noBreakAbbreviations` options apply to the `"sentence"` and `"semantic"` wrap modes.

Badness always indents with spaces, so the global `useTabs` setting has no effect.

Configuration keys use camelCase. Option values keep the spelling used in badness.toml, such as `"single-line"`.
See the [Badness configuration reference](https://badness.dev/reference/configuration.html),
[plugin documentation](https://github.com/jolars/dprint-plugin-badness#configuration), and
[JSON schema](https://plugins.dprint.dev/jolars/badness/latest/schema.json) for details.
