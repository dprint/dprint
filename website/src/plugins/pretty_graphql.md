---
title: Pretty GraphQL Plugin
description: Documentation on the GraphQL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/pretty_graphql">Pretty GraphQL</a></li>
  </ul>
</nav>

# Pretty GraphQL Plugin

Adapter plugin that formats GraphQL files via [Pretty GraphQL](https://github.com/g-plane/pretty_graphql).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint config add g-plane/pretty_graphql
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"graphql"` property to add configuration:

```json
{
  "graphql": { // not "pretty_graphql"
    // Pretty GraphQL config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/g-plane/pretty_graphql-vx.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/pretty_graphql/config) or read [full documentation site](https://pretty-graphql.netlify.app/) with code examples.

## Playground

See [Playground](https://dprint.dev/playground#plugin/pretty_graphql)
