---
title: Rustfmt Plugin
description: Documentation on the rustfmt code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/rustfmt">Rustfmt</a></li>
  </ul>
</nav>

# Rustfmt Plugin

Wrapper plugin that formats Rust code via [rustfmt](https://github.com/rust-lang/rustfmt).

## Install and Setup

Specify the plugin url in _dprint.config.json_ and add a `"rustfmt"` configuration property if desired:

```json
{
    // ...etc...
    "rustfmt": {
        // rustfmt config goes here
        "brace_style": "AlwaysNextLine"
    },
    "plugins": [
        // ...etc...
        "https://plugins.dprint.dev/rustfmt-x.x.x.wasm"
    ]
}
```

## Configuration

See documentation [here](https://rust-lang.github.io/rustfmt/).
