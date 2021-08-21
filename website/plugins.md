---
title: Plugins
description: Links to dprint formatting plugins.
---

# Plugins

Dprint is made up of Wasm and process plugins.

- Wasm plugins are compiled to a `.wasm` file and run sandboxed.
- Process plugins are compiled to an executable file and do not run sandboxed.

It would be ideal for all plugins to be Wasm plugins, but unfortunately many languages don't support compiling to a single `.wasm` file. Until then, process plugins exist.

The setup for both is the same except process plugins require a checksum to be specified to ensure the downloaded file is the same as what was built on the CI pipeline.

## Wasm Plugins

- [Typescript / JavaScript](/plugins/typescript)
- [JSON](/plugins/json)
- [Markdown](/plugins/markdown)
- [TOML](/plugins/toml)

## Process Plugins

- [Prettier](/plugins/prettier)
- [Roslyn](/plugins/roslyn) (C#/VB)
- [Rustfmt](/plugins/rustfmt)

## Using Wasm Plugins in the Browser, Deno, or Node.js

See https://github.com/dprint/js-formatter

- Import _mod.ts_ in [https://deno.land/x/dprint](https://deno.land/x/dprint) for Deno or the browser.
- Use the [@dprint/formatter](https://www.npmjs.com/package/@dprint/formatter) package in npm for Node.js.
- [Documentation](https://doc.deno.land/https/deno.land/x/dprint/mod.ts)

Deno/Browser example:

```ts
// see current version at https://github.com/dprint/js-formatter/releases
import { createStreaming } from "https://deno.land/x/dprint@x.x.x/mod.ts";

const globalConfig = {
  indentWidth: 2,
  lineWidth: 80,
};
const tsFormatter = await createStreaming(
  fetch("https://plugins.dprint.dev/typescript-x.x.x.wasm"),
);

tsFormatter.setConfig(globalConfig, {
  semiColons: "asi",
});

// outputs: "const t = 5\n"
console.log(tsFormatter.formatText("file.ts", "const   t    = 5;"));
```
