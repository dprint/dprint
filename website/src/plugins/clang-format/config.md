---
title: Configuration - clang-format
description: Documentation on the configuration file for the clang-format code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/clang-format">clang-format</a></li>
    <li><a href="/plugins/clang-format/config">Configuration</a></li>
  </ul>
</nav>

# clang-format - Configuration

Specify configuration under the `"clangFormat"` key in your dprint configuration file, using clang-format's own option names represented as JSON:

```json
{
  "clangFormat": {
    "BasedOnStyle": "LLVM",
    "ColumnLimit": 100,
    "IndentWidth": 2,
    "SpaceBeforeParens": "Custom",
    "SpaceBeforeParensOptions": {
      "AfterControlStatements": false,
      "AfterFunctionDeclarationName": true
    }
  }
}
```

See the [clang-format style options](https://clang.llvm.org/docs/ClangFormatStyleOptions.html) for the available options.

## Global Configuration

The following global options are used when the matching clang-format option is not set under `"clangFormat"`:

| Global option | clang-format option |
| ------------- | ------------------- |
| `lineWidth`   | `ColumnLimit`       |
| `indentWidth` | `IndentWidth`       |
| `useTabs`     | `UseTab`            |

## Limitations

- The plugin runs sandboxed and cannot read `.clang-format` files, so `"BasedOnStyle": "file"` and `"BasedOnStyle": "InheritParentConfig"` are rejected. Put the style in your dprint configuration instead.
- Files whose nested `#if`/`#ifdef`/`#elif` branches produce more than 16 branch combinations may format differently from native clang-format in inactive branches.

See the [plugin documentation](https://github.com/sargunv/dprint-clang-format#configure) and
[JSON schema](https://plugins.dprint.dev/sargunv/dprint-clang-format/latest/schema.json) for details.
