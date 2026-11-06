---
title: Malva Plugin
description: Documentation on the Malva code formatting plugin for dprint.
layout: layouts/plugin.njk
---

Plugin that formats CSS, SCSS, Sass (indented syntax), and Less files via [Malva](https://github.com/g-plane/malva).

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add g-plane/malva
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"malva"` property to add configuration:

```json
{
  "malva": {
    // malva config goes here
  },
  "plugins": [
    "npm:dprint-plugin-malva@x.x.x"
  ]
}
```

## Configuration

Read the [full documentation site](https://malva.netlify.app/) with code examples.
