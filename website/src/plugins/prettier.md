---
title: Prettier Plugin
description: Documentation on the Prettier code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Prettier Plugin

Adapter plugin that formats [many languages](https://prettier.io/docs/en/index.html) via [Prettier](https://prettier.io).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

```shellsession
dprint add prettier
# or install from npm
dprint add npm:@dprint/prettier
```

## Configuration

See Prettier's configuration [here](https://prettier.io/docs/en/options.html). Specify using the "API Override" column.
