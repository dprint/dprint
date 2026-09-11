---
title: Configuration - bibtex-tidy
description: Documentation on the configuration file for the bibtex-tidy code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/bibtex-tidy">bibtex-tidy</a></li>
    <li><a href="/plugins/bibtex-tidy/config">Configuration</a></li>
  </ul>
</nav>

# bibtex-tidy - Configuration

Specify configuration under the `"bibtex-tidy"` key in your dprint configuration file.

| Property                | Values                                                               | Default                         |
| ----------------------- | -------------------------------------------------------------------- | ------------------------------- |
| `lineWidth`             | Integer                                                              | Global `lineWidth`, or `80`     |
| `indentWidth`           | Integer                                                              | Global `indentWidth`, or `2`    |
| `newLineKind`           | `"auto"`, `"lf"`, `"crlf"`, `"system"`                               | Global `newLineKind`, or `"lf"` |
| `useTabs`               | Boolean                                                              | Global `useTabs`, or `false`    |
| `align`                 | Integer, Boolean                                                     | `14`                            |
| `blankLines`            | Boolean                                                              | `false`                         |
| `curly`                 | Boolean                                                              | `false`                         |
| `numeric`               | Boolean                                                              | `false`                         |
| `months`                | Boolean                                                              | `false`                         |
| `sort`                  | Boolean, Array of strings                                            | `false`                         |
| `duplicates`            | Boolean, Array of `"doi"` \| `"key"` \| `"abstract"` \| `"citation"` | `false`                         |
| `merge`                 | Boolean, `"first"`, `"last"`, `"combine"`, `"overwrite"`             | `false`                         |
| `stripEnclosingBraces`  | Boolean                                                              | `false`                         |
| `dropAllCaps`           | Boolean                                                              | `false`                         |
| `escape`                | Boolean, `"new"`                                                     | `true`                          |
| `unescape`              | Boolean                                                              | `false`                         |
| `sortFields`            | Boolean, Array of strings                                            | `false`                         |
| `stripComments`         | Boolean                                                              | `false`                         |
| `tidyComments`          | Boolean                                                              | `true`                          |
| `trailingCommas`        | Boolean                                                              | `false`                         |
| `encodeUrls`            | Boolean                                                              | `false`                         |
| `removeEmptyFields`     | Boolean                                                              | `false`                         |
| `removeDuplicateFields` | Boolean                                                              | `true`                          |
| `generateKeys`          | Boolean, String                                                      | `false`                         |
| `maxAuthors`            | Integer                                                              | Unset                           |
| `lowercase`             | Boolean                                                              | `true`                          |
| `enclosingBraces`       | Boolean, Array of strings                                            | `false`                         |
| `removeBraces`          | Boolean, Array of strings                                            | `false`                         |
| `wrap`                  | Boolean                                                              | `false`                         |
| `omit`                  | Array of strings                                                     | `[]`                            |

`lineWidth` is only used when `wrap` is set to `true`. When `useTabs` is `true`, `indentWidth` is ignored.

Setting `enclosingBraces` or `removeBraces` to `true` applies them to the `title` field. `generateKeys` is experimental in bibtex-tidy.

See the [plugin documentation](https://github.com/apcamargo/dprint-plugin-bibtex-tidy#configuration) and
[JSON schema](https://plugins.dprint.dev/apcamargo/bibtex-tidy/latest/schema.json) for details.
