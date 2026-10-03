---
title: Lax SQL Plugin
description: Documentation on the Lax SQL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/lax-sql">Lax SQL</a></li>
  </ul>
</nav>

# Lax SQL Plugin

Formats SQL files via [lax-sql](https://github.com/bartlomieju/lax/tree/main/crates/lax-sql), a dialect-agnostic formatter that splits statements into clauses and never rewrites a token.

Formats .sql files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add bartlomieju/lax-sql
# or install from npm
dprint add npm:lax-sql
```

This will update your config file to have an entry for the plugin. Then optionally specify an `"sql"` property to add configuration:

```jsonc
{
  "sql": {
    // sql config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/bartlomieju/lax-sql-x.x.x.wasm"
  ]
}
```

## Configuration

See the [plugin documentation](https://github.com/bartlomieju/lax/tree/main/crates/lax-sql#configuration).
