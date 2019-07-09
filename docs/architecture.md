# Architecture

The source code is obfuscated (I'm not sure I will open source this), but here's a brief overview...

1. Source code is parsed to an AST.
2. AST is parsed to a "print tree".
3. Print tree is printed by a printer.

## AST Parsing

Done with the Babel compiler.

I didn't use the TypeScript compiler because Babel's AST gives more information that's useful for formatting purposes. If I were to use the TypeScript compiler I would have had to write a bunch of performant utility functions for dealing with a lot of stuff Babel easily provides (ex. line and column numbers for every node, tokens, easier to handle leading/trailing/inner comments).

Major downside to doing this is that it will take longer to support new TypeScript syntax.

## Print Tree

The print tree allows writing simplish declarativish code describing how the nodes should be formatted.

It's a rather flat tree with the following nodes:

1. Texts.
2. Infos.
3. Conditions.
4. Signals.

### Text nodes

This is just a string that the printer should print. For example `"async"`.

### Info nodes

These nodes can be put into the tree and when resolved, report information such as the line number and column it appears at.

### Conditions

These conditions have an optional true and optional false path based on how the condition is resolved. Conditions are usually resolved by looking at the value of a resolved info node, which could appear before or after the condition (look ahead).

### Signals

This is an enum that signals information such as when a newline might occur due to the print width exceeeding, where hanging indent should start/stop, where indent should start/stop, and a handful of other things.

## Printer

The printer turns the print tree into text. It goes through the tree resolving infos and conditions.

The most complicated code is in the printer, but luckily not a lot of time is spent maintaining it because it works within a small domain.
