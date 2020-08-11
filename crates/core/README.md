# dprint-core

[![](https://img.shields.io/crates/v/dprint-core.svg)](https://crates.io/crates/dprint-core)

Rust crate for common dprint code.

Features:

- `formatting` - Code to help build a code formatter in Rust (not required for creating a plugin).
- `process` - Code to help build a "process plugin"
- `wasm` - Code to help build a "wasm plugin" (recommended over process plugins)

## Api

Use:

```rust
let print_items = ...; // parsed out IR (see example below)
let result = dprint_core::formatting::print(print_items, PrintOptions {
    indent_width: 4,
    max_width: 10,
    use_tabs: false,
    newline_kind: "\n",
});
```

## Example

See [overview.md](../../docs/overview.md).
