# Architecture

1. Source code is parsed to an AST.
2. AST is traversed and generates IR.
3. IR is printed by a printer.

## AST Parsing

Done with the Babel compiler.

I didn't use the TypeScript compiler because Babel's AST gives more information that's useful for formatting purposes. If I were to use the TypeScript compiler I would have had to write a bunch of performant utility functions for dealing with a lot of stuff Babel easily provides (ex. line and column numbers for every node, tokens, easier to handle leading/trailing/inner comments).

Major downside to doing this is that it will take slightly longer to support new TypeScript syntax, but other formatters currently have this problem as well.
