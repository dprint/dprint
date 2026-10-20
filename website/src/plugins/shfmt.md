---
title: shfmt Plugin
description: Documentation on the shfmt (shell script) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# shfmt Plugin

Wrapper plugin that formats shell scripts via [shfmt](https://github.com/mvdan/sh). It runs shfmt's own code compiled to Wasm, so it formats exactly like the shfmt CLI.

Formats .sh, .bash, .mksh, .bats, and .zsh files, along with shell dotfiles such as .bashrc, .bash_profile, .zshrc, and .zshenv. POSIX shell, Bash, mksh, Bats, and Zsh are supported.

See also the [Shuck plugin](/plugins/shuck), which formats shell scripts with shuck-formatter, a separate implementation in Rust.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add shfmt
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"shfmt"` property to add configuration:

```jsonc
{
  "shfmt": {
    // shfmt's config goes here
  },
  "plugins": [
    "npm:@dprint/shfmt@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/shfmt/config).

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

See https://github.com/dprint/dprint-plugin-shfmt
