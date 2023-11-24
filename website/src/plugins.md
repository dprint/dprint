---
title: Plugins
description: Links to dprint formatting plugins.
layout: layouts/documentation.njk
---

# Plugins

Dprint is made up of Wasm and process plugins.

- Wasm plugins are compiled to a `.wasm` file and run sandboxed.
- Process plugins are compiled to an executable file and do NOT run sandboxed.

It would be ideal for all plugins to be Wasm plugins, but unfortunately many languages don't support compiling to a single `.wasm` file. Until then, process plugins exist.

The setup for both is the same except process plugins require a checksum to be specified to ensure the downloaded file is the same as what was built on the CI pipeline.

## Wasm Plugins

- [Typescript / JavaScript](/plugins/typescript)
- [JSON](/plugins/json)
- [Markdown](/plugins/markdown)
- [TOML](/plugins/toml)
- [Dockerfile](/plugins/dockerfile)
- [Biome](/plugins/biome) (JS/TS/JSON)
- [Ruff](/plugins/ruff) (Python)
- [Jupyter](/plugins/juypter)

## Process Plugins

- [Prettier](/plugins/prettier)
- [Roslyn](/plugins/roslyn) (C#/VB)
- [Exec](/plugins/exec) - Works with any formatting CLI installed on the system.

## Using Wasm Plugins in the Browser, Deno, or Node.js

See https://github.com/dprint/js-formatter
