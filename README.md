# dprint

This library is currently under construction, but will serve as the formatter for all my TypeScript projects. Not recommended for use yet.

```ts
import { formatFileText } from "dprint";

const fileText = `if(condition){callExpr( true,false\n   )}`;
const formattedText = formatFileText("file.ts", fileText);

// outputs:
// if (condition)
//     callExpr(true, false);
console.log(fileText);
```

TODO:

Add back keywords `"printer", "formatter", "typescript", "javascript"` to package.json.