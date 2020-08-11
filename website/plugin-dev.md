---
title: Creating a Plugin
description: Documentation on creating your own dprint formatting plugin.
---

# Creating a Plugin

As outlined in [plugins](/plugins), there are WASM plugins and process plugins.

- WASM plugins can be written in any language that supports compiling to a WebAssembly file (_.wasm_) (highly recommended)
- Process plugins can be written in any language that supports compiling to an executable.

Links:

- [Wasm plugin development](https://github.com/dprint/dprint/blob/master/docs/wasm-plugin-development.md)
- [Process plugin development](https://github.com/dprint/dprint/blob/master/docs/process-plugin-development.md)

Note that plugins only need to conform to a general interface that doesn't prescribe a certain way of implementing the formatter. In Rust, you may want to use the `dprint-core` crate's [`formatting`](https://docs.rs/dprint-core/0.28.0/dprint_core/formatting/index.html) feature as it provides a better starting point for implementing a formatter. See an overview [here](https://github.com/dprint/dprint/blob/master/docs/overview.md)
