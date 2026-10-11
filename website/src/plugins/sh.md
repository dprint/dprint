---
title: Shell Plugin
description: Documentation on the Shell code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/sh">Shell</a></li>
  </ul>
</nav>

# Shell Plugin

Adapter plugin that formats shell scripts via [shuck-formatter](https://github.com/ewhauser/shuck).

Formats .sh, .bash, .zsh, .ksh, .mksh, .dash, and .bats files, along with shell dotfiles such as .envrc, .bashrc, .bash_profile, .profile, .zshrc, and .zshenv.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add kachick/sh
```

This will update your config file to have an entry for the plugin. Then optionally specify an `"sh"` property to add configuration:

```jsonc
{
  "sh": {
    // sh config goes here
  },
  "plugins": [
    "npm:@kachick/dprint-plugin-sh@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/sh/config).

## Formatting Shell Code Blocks in Markdown

Use the `tags` option of the [Markdown plugin](/plugins/markdown) to map a code block's language tag to a file extension this plugin formats:

```json
{
  "markdown": {
    "tags": {
      "sh": "sh",
      "bash": "bash",
      "zsh": "zsh",
      "shell": "sh"
    }
  }
}
```
