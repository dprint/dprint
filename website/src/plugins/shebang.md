---
title: Shebang Plugin
description: Documentation on the Shebang code formatting plugin for dprint.
layout: layouts/plugin.njk
---

Formats the [shebang](https://en.wikipedia.org/wiki/Shebang_%28Unix%29) line at the start of executable scripts. This is an additive plugin, so it can be used alongside other formatters that format the rest of the file.

The plugin provides no configuration options.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add scop/shebang
```

Scripts with common filename extensions and whole filenames (for example, `Makefile`) are associated automatically. For extensionless scripts, add an [association](/config/#associations) so dprint knows to format them:

```jsonc
{
  "associations": {
    "my-script": "shebang"
  },
  "plugins": [
    "npm:dprint-plugin-shebang@x.x.x"
  ]
}
```

See the [plugin repository](https://github.com/scop/dprint-plugin-shebang) for more details.
