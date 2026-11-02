# dprint-core

[![](https://img.shields.io/crates/v/dprint-core.svg)](https://crates.io/crates/dprint-core)

Rust crate for common dprint code.

Features:

- `formatting` - Code to help build a code formatter in Rust (not required for creating a plugin).
- `process` - Code to help build a "process plugin"
- `wasm` - Code to help build a "wasm plugin" (recommended over process plugins)

## Formatting Api

Use:

<!-- dprint-ignore -->
```rust
// create the arena after the data the print items borrow from
let arena = Arena::new();
let print_items = ...; // parsed out IR allocated in the arena (see example below)
let result = dprint_core::formatting::print(print_items, PrintOptions {
    indent_width: 4,
    max_width: 10,
    use_tabs: false,
    new_line_text: "\n",
});
```

## Example

See [overview.md](../../docs/overview.md).
