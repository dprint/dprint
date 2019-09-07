# Architecture

1. Source code is parsed to an AST.
2. AST is traversed and creates a "print tree".
3. Print tree is printed by a printer.

## AST Parsing

Done with the Babel compiler.

I didn't use the TypeScript compiler because Babel's AST gives more information that's useful for formatting purposes. If I were to use the TypeScript compiler I would have had to write a bunch of performant utility functions for dealing with a lot of stuff Babel easily provides (ex. line and column numbers for every node, tokens, easier to handle leading/trailing/inner comments).

Major downside to doing this is that it will take slightly longer to support new TypeScript syntax, but other formatters currently have this problem as well.

## Print Tree

The print tree allows writing simplish declarativish code describing how the nodes should be formatted.

It's actually not much of a tree and is quiet shallow. It consists of the following nodes:

1. Texts.
2. Infos.
3. Conditions.
4. Signals.

The nodes of the tree are streamed to the printer using generators.

### Text nodes

This is just a string that the printer should print. For example `"async"`.

### Info nodes

These nodes can be put into the tree and when resolved, report information such as the line number and column it appears at.

### Conditions

These conditions have an optional true and optional false path based on how the condition is resolved. Conditions are usually resolved by looking at the value of an info node or other condition. These infos/conditions that are inspected can appear before or even after the condition.

### Signals

This is an enum that signals information such as when a newline might occur due to the print width exceeeding, where hanging indent should start/stop, where indent should start/stop, and a handful of other things.

## Printer

The printer turns the print tree into text. It goes through the tree resolving infos and conditions.

The most complicated code is in the printer, but luckily not a lot of time is spent maintaining it because it works within a small domain.
