---
title: Nix Plugin
description: Documentation on the Nix code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/nix">Nix</a></li>
  </ul>
</nav>

# Nix Plugin

Adapter plugin that formats Nix code via [nixfmt-rs](https://github.com/Mic92/nixfmt-rs), a Rust port of [nixfmt](https://github.com/NixOS/nixfmt).

Formats .nix files.

<div class="message is-warning">
  <div class="message-body">
    This plugin is updated after upstream nixfmt and nixfmt-rs, so its output can lag behind the latest nixfmt. The plugin author recommends against using it for nixpkgs or other NixOS organization contributions.
  </div>
</div>

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add kachick/nix
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"nix"` property to add configuration:

```jsonc
{
  "nix": {
    // nix config goes here
  },
  "plugins": [
    "npm:@kachick/dprint-plugin-nix@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/nix/config).

## Formatting Nix Code Blocks in Markdown

Use the `tags` option of the [Markdown plugin](/plugins/markdown) to format `nix` code blocks with this plugin:

```json
{
  "markdown": {
    "tags": {
      "nix": "nix"
    }
  }
}
```
