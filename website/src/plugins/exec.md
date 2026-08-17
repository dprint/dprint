---
title: Exec Plugin
description: Documentation on the Exec code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Exec Plugin

Plugin that formats code via mostly any formatting CLI found on the host machine.

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install, Setup, and Configuration

```shellsession
dprint add exec
# or install from npm
dprint add npm:@dprint/exec
```

See further setup and configuration instructions at [https://github.com/dprint/dprint-plugin-exec/](https://github.com/dprint/dprint-plugin-exec/).
