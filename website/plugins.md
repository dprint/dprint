---
title: Plugins
description: Links to dprint formatting plugins.
---

# Plugins

These are the only plugins available at this time:

- [Typescript / JavaScript](/plugins/typescript)
- [JSON](/plugins/json)
- [Rustfmt](/plugins/rustfmt)
- More to come!

## Using Plugins in Deno

Plugins may be used to format text in Deno like so:

```ts
import { createStreaming } from "https://dprint.dev/formatter/v1.ts";

const globalConfig = {
    indentWidth: 2,
    lineWidth: 80,
};
const tsFormatter = await createStreaming(
    fetch("https://plugins.dprint.dev/typescript-x.x.x.wasm")
);

tsFormatter.setConfig(globalConfig, {
    semiColons: "asi",
});

// outputs: "const t = 5\n"
console.log(tsFormatter.formatText("file.ts", "const   t    = 5;"));
```

Notes:

- [Documentation](https://doc.deno.land/https/dprint.dev/formatter/v1.ts)
- Make sure to check the license of a plugin when you use it to see if use is permitted this way. You may read a plugin's license text by running `#getLicenseText()` on the returned formatter object. For example, `tsFormatter.getLicenseText()` in this case returns the MIT license.
