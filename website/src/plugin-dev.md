---
title: Creating a Plugin
description: Documentation on creating your own dprint formatting plugin.
layout: layouts/plugins_page.njk
---

# Creating a Plugin

As outlined in [plugins](/plugins), there are Wasm plugins and process plugins.

- Wasm plugins can be written in any language that supports compiling to a WebAssembly file (_.wasm_) (highly recommended)
- Process plugins can be written in any language that supports compiling to an executable.

Links:

- [Wasm plugin development](https://github.com/dprint/dprint/blob/main/docs/wasm-plugin-development.md)
- [Process plugin development](https://github.com/dprint/dprint/blob/main/docs/process-plugin-development.md)

Note that plugins only need to conform to a general interface that doesn't prescribe a certain way of implementing the formatter. In Rust, you may want to use the `dprint-core` crate's [`formatting`](https://docs.rs/dprint-core/0.28.0/dprint_core/formatting/index.html) feature as it provides a better starting point for implementing a formatter. See an overview [here](https://github.com/dprint/dprint/blob/main/docs/overview.md)

## Package a Wasm plugin for npm

The dprint CLI looks for `plugin.wasm` at the root of an npm package by default. Put your compiled Wasm plugin there and add a `package.json`:

```json
{
  "name": "@example/sample-format",
  "version": "0.1.0",
  "files": ["plugin.wasm"]
}
```

The `files` list includes the Wasm artifact in the tarball. You don't need a JavaScript entry point for dprint to load the plugin.

To test the package without publishing it:

1. Run `npm pack --dry-run --ignore-scripts` in the package directory to check that `plugin.wasm` is included, then run `npm pack --ignore-scripts` to create the tarball.
2. In a separate test project, install the tarball with `npm install --ignore-scripts --save-dev ../path/to/example-sample-format-0.1.0.tgz`.
3. Add `"npm:@example/sample-format"` to the `plugins` array in `dprint.json` and run `dprint fmt` on a file your plugin supports.

If you put the artifact at a different path, such as `dist/formatter.wasm`, include it in `files` and specify the path as `npm:@example/sample-format/dist/formatter.wasm`.

Once you [publish the package](https://docs.npmjs.com/cli/v11/commands/npm-publish/), users can pin it with `npm:@example/sample-format@0.1.0`. See [npm plugin specifiers](/config/#plugins) for registry and local resolution details.
