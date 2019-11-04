# rust-printer

```
yarn add @dprint/rust-printer
```

Uses the [`dprint-core` crate](../rust-core) living in WebAssembly to print out JavaScript ["print items"](https://github.com/dsherret/dprint/blob/master/docs/overview.md) (the IR).

* [Api Declarations](lib/dprint-rust-printer.d.ts)

## Example

Example use with `@dprint/core`:

```ts
import { printer as rustPrinter } from "@dprint/rust-printer";
import { formatFileText, resolveConfiguration } from "@dprint/core";
import { TypeScriptPlugin } from "dprint-plugin-typescript";

const typeScriptPlugin = new TypeScriptPlugin({
    /* config goes here */
    lineWidth: 80
});

const formattedText = formatFileText({
    filePath: "/file.ts",
    fileText: "class Test {prop: string;}",
    plugins: [typeScriptPlugin],
    customPrinter: rustPrinter
});
```

Alternatively, use the exported `print` function and provide your own parsed out print items. See some example IR generation in [the overview](../../docs/overview.md#example-ir-generation).