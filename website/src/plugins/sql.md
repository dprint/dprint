---
title: SQL Plugin
description: Documentation on the SQL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/sql">SQL</a></li>
  </ul>
</nav>

# SQL Plugin

Adapter plugin that formats SQL code via [sqlformat-rs](https://github.com/shssoichiro/sqlformat-rs).

Formats .sql files.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add sql
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"sql"` property to add configuration:

```json
{
  "sql": {
    // sql config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/sql-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/sql/config)

## Source

See https://github.com/dprint/dprint-plugin-sql
