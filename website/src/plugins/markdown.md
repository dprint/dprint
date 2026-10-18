---
title: Markdown Plugin
description: Documentation on the Markdown code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Markdown Code Formatter

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add markdown
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"markdown"` property to add configuration:

```json
{
  "markdown": {
    // markdown config goes here
  },
  "plugins": [
    "npm:@dprint/markdown@x.x.x"
  ]
}
```

## Code block formatters

Code blocks are formatted based on the other provided plugins. For example, if you wish to format JSON, TypeScript, and JavaScript code blocks, then ensure those plugins are also specified in the list of plugins to use.

```json
{
  "plugins": [
    "npm:@dprint/typescript@x.x.x",
    "npm:@dprint/json@x.x.x",
    "npm:@dprint/markdown@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/markdown/config)

## Playground

See [Playground](https://dprint.dev/playground#plugin/markdown)

## Ignore Comments

Use an ignore comment:

<!-- dprint-ignore -->

```md
<!-- dprint-ignore -->
Some              text
```

Or a range ignore:

<!-- dprint-ignore -->

```md
<!-- dprint-ignore-start -->

Some    text

* other    text
*           testing

<!-- dprint-ignore-end -->
```
