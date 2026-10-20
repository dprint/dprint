---
title: Shuck Plugin
description: Documentation on the Shuck (shell script) code formatting plugin for dprint.
layout: layouts/documentation.njk
oldUrl: /plugins/sh/
---

# Shuck Plugin

Wrapper plugin that formats shell scripts via [shuck-formatter](https://github.com/ewhauser/shuck).

Formats .sh, .bash, .zsh, .ksh, .mksh, .dash, and .bats files, along with shell dotfiles such as .envrc, .bashrc, .bash_profile, .profile, .zshrc, and .zshenv.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add shuck
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"shuck"` property to add configuration:

```jsonc
{
  "shuck": {
    // shuck config goes here
  },
  "plugins": [
    "npm:@dprint/shuck@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/shuck/config).

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

## Source

See https://github.com/dprint/dprint-plugin-shuck
