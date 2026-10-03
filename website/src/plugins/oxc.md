---
title: Oxc Plugin
description: Documentation on the Oxc code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Oxc Plugin

Adapter plugin that formats files via [Oxc](https://oxc.rs). It formats the languages that Oxc's formatter formats natively in Rust:

- JavaScript and TypeScript (including JSX)
- JSON, JSONC, and JSON5
- CSS, SCSS, and Less
- GraphQL
- YAML
- TOML

Code embedded in these files is also formatted (ex. CSS or GraphQL in a JavaScript template literal).

Markdown is also supported, but is opt-in because Oxc's own formatter does not use its Markdown formatter yet. To enable it, set `"experimentalMarkdown": true` in the plugin's configuration.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add oxc
# or install from npm
dprint add npm:@dprint/oxc
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"oxc"` property to add configuration:

```json
{
  "oxc": {
    // oxc's config goes here
  },
  "plugins": [
    "https://plugins.dprint.dev/oxc-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/oxc/config)

The configuration is shared between the languages. For example, `quoteStyle` applies to JavaScript, CSS, YAML, and so on.

## Using with other plugins

This plugin formats many kinds of files, so it may match the same files as another plugin in your config file. The order of the `"plugins"` array defines the precedence, so list a plugin before this one to have it format the files they both match:

```json
{
  "plugins": [
    // formats .json files instead of the Oxc plugin
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/oxc-x.x.x.wasm"
  ]
}
```

For more fine grained control, see [associations](/config/#associations).

## Playground

See [Playground](https://dprint.dev/playground#plugin/oxc)

## Source

See https://github.com/dprint/dprint-plugin-oxc
