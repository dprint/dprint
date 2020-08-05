---
title: Plugins
description: Links to dprint formatting plugins.
---

# Plugins

Dprint is made up of WASM and process plugins.

- WASM plugins are compiled to a `.wasm` file and run sandboxed.
- Process plugins are compiled to an executable file and do not run sandboxed.

It would be ideal for all plugins to be WASM plugins, but unfortunately many languages don't support compiling to a single `.wasm` file. Until then, process plugins exist.

The setup for both is the same except process plugins require a checksum to be specified to ensure the downloaded file is the same as what was built on the CI pipeline.

## WASM Plugins

- [Typescript / JavaScript](/plugins/typescript)
- [JSON](/plugins/json)
- [Markdown](/plugins/markdown)
- [Rustfmt](/plugins/rustfmt)

## Process Plugins

- [Roslyn](/plugins/roslyn) (C#/VB)
- [Prettier](/plugins/prettier)

## Using WASM Plugins in Deno

WASM plugins may be used to format text in Deno like so:

```ts
import { createStreaming } from "https://dprint.dev/formatter/v2.ts";

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

Notes:

- [Documentation](https://doc.deno.land/https/dprint.dev/formatter/v2.ts)
- Make sure to check the license of a plugin when you use it to see if use is permitted this way. You may read a plugin's license text by running `#getLicenseText()` on the returned formatter object. For example, `tsFormatter.getLicenseText()` in this case returns the MIT license.
