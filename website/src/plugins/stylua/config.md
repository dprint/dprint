---
title: Configuration - StyLua
description: Documentation on the configuration file for the StyLua (Lua and Luau) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# StyLua - Configuration

The options are StyLua's, with the same defaults. `lineWidth`, `useTabs`, `indentWidth`, and `newLineKind` use the global configuration when not specified, and otherwise default to StyLua's 120 columns, tabs with a width of 4, and LF newlines. The plugin doesn't read `stylua.toml` or `.editorconfig` files, so configure it in your dprint config instead.

See the [plugin documentation](https://github.com/dprint/dprint-plugin-stylua#configuration) for more details.

<div class="plugin-config-table" data-url="https://plugins.dprint.dev/dprint/dprint-plugin-stylua/latest/schema.json">
  Loading...
</div>
