---
title: Swift Plugin
description: Documentation on the Swift code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Swift Plugin

Adapter plugin that formats Swift code via bundled [SwiftFormat](https://github.com/nicklockwood/SwiftFormat).

Formats `.swift` files on macOS and Linux (x86_64 and aarch64).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add drluckyspin/swift
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"swiftformat"` property to add configuration:

```json
{
  "swiftformat": {
    "swiftVersion": "5.9"
  },
  "plugins": [
    "https://plugins.dprint.dev/drluckyspin/swift-vx.x.x.json@<checksum>"
  ]
}
```

Update to the latest release with `dprint config update`.

## Configuration

See the plugin repository for configuration options and the JSON schema:

- [dprint-plugin-swift](https://github.com/drluckyspin/dprint-plugin-swift)
- [schema.json](https://plugins.dprint.dev/drluckyspin/dprint-plugin-swift/v0.1.0/schema.json)

You can also keep a `.swiftformat` file in your project. The plugin passes `--stdinpath` so SwiftFormat discovers it automatically.
