---
title: YAPF Plugin
description: Documentation on the YAPF code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/yapf">YAPF</a></li>
  </ul>
</nav>

# YAPF Plugin

Wrapper plugin that formats python files via [YAPF](https://github.com/google/yapf).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

Follow the instructions at [https://github.com/dprint/dprint-plugin-yapf/releases/](https://github.com/dprint/dprint-plugin-yapf/releases/)

## Configuration

See YAPF's configuration [here](https://github.com/google/yapf#knobs) and specify the `"yapf"` property in your dprint configuration file.

```jsonc
{
  // ...etc...
  "yapf": {
    "based_on_style": "pep8",
    "spaces_before_comment": 4
  }
}
```
