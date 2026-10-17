---
title: Configuration - clang-format
description: Documentation on the configuration file for the clang-format code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# clang-format - Configuration

The configuration takes [clang-format's style options](https://clang.llvm.org/docs/ClangFormatStyleOptions.html) with their clang-format names, represented as JSON:

```json
{
  "clangFormat": {
    "BasedOnStyle": "LLVM",
    "ColumnLimit": 100,
    "SpaceBeforeParens": "Custom",
    "SpaceBeforeParensOptions": {
      "AfterControlStatements": false
    }
  }
}
```

The global `lineWidth`, `indentWidth`, and `useTabs` properties are used for `ColumnLimit`, `IndentWidth`, and `UseTab` when those aren't specified. clang-format's configuration files (_.clang-format_) aren't read, so `"BasedOnStyle": "file"` and `"BasedOnStyle": "InheritParentConfig"` aren't supported.

<div class="plugin-config-table" data-url="https://plugins.dprint.dev/dprint/dprint-plugin-clang-format/latest/schema.json">
  Loading...
</div>
