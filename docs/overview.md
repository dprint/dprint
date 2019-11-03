# Overview

1. Source code is parsed to an AST (recommended, but not required).
2. AST is traversed and an IR is generated.
3. IR is printed by a printer.

## IR Generation

The immediate representation describes how the nodes should be formatted. It consists of...

1. Texts
2. Infos
3. Conditions
4. Signals

These are referred to as "print items" in the code.

### Texts

Strings that the printer should print. For example `"async"`.

### Infos

These objects are invisible in the output. They may be placed into the IR and when resolved by the printer, report the following information about where the info ended up at:

* `lineNumber`
* `columnNumber`
* `indentLevel`
* `lineStartIndentLevel`
* `lineStartColumnNumber`

### Conditions

Conditions have three main properties:

* Optional true path - Print items to use when the condition is resolved as *true*.
* Optional false path - Print items to use when the condition is resolved as *false*.
* Condition resolver - Function or condition that the printer uses to resolve the condition as *true* or *false*.

#### Condition Resolver

Conditions are usually resolved by looking at the value of a resolved info, other condition, or based on the original AST node.

The infos & conditions that are inspected may appear before or even after the condition.

### Signals

This is an enum that signals information to the printer.

* `NewLine` - Signal that a new line should occur based on the printer settings.
* `PossibleNewLine` - Signal that the current location could be a newline when exceeding the line width.
* `SpaceOrNewLine` - Signal that the current location should be a space, but could be a newline if exceeding the line width.
* `ExpectNewLine` - Expect the next character to be a newline. If it's not, force a newline. This is useful to use at the end of single line comments in JS, for example.
* `StartIndent` - Signal the start of a section that should be indented.
* `FinishIndent` - Signal the end of a section that should be indented.
* `StartNewLineGroup` - Signal the start of a group of print items that have a lower precedence for being broken up with a newline for exceeding the line width.
* `FinishNewLineGroup` - Signal the end of a newline group.
* `SingleIndent` - Signal that a single indent should occur based on the printer settings (ex. prints a tab when using tabs).
* `StartIgnoringIndent` - Signal to the printer that it should stop using indentation.
* `FinishIgnoringIndent` - Signal to the printer that it should start using indentation again.

## Printer

The printer takes the IR and outputs the final code. Its main responsibilities are:

1. Resolving infos and conditions in the IR.
2. Printing out the text with the correct indentation and newline kind.
3. Seeing where lines exceed the maximum line width and breaking up the line as specified in the IR.

* [Printer code](../packages/core/src/printing/printer.ts)
* [Writer code](../packages/core/src/printing/Writer.ts) - Simple code writer used by the printer.

## Example IR Generation

Given the following example AST nodes:

```ts
enum SyntaxKind {
    ArrayLiteralExpression,
    ArrayElement
}

interface BaseNode {
    kind: SyntaxKind;
    /** Line number in the original source code. */
    lineNumber: number;
    /** Column number in the original source code. */
    columnNumber: number;
}

type Node = ArrayLiteralExpression | ArrayElement;

interface ArrayLiteralExpression extends BaseNode {
    kind: SyntaxKind.ArrayLiteralExpression;
    elements: ArrayElement[];
}

interface ArrayElement extends BaseNode {
    kind: SyntaxKind.ArrayElement;
    text: string;
}
```

With the following expected outputs (when max line width configured in printer is 10):

```ts
// input
[a   ,   b
    , c
   ]
// output
[a, b, c]

// input
[four, four, four]
// output (since it exceeds the line width of 10)
[
    four,
    four,
    four
]

// input
[
four]
// output (since first element was placed on a different line)
[
    four
]
```

Here's some example IR generation:

```ts
import { PrintItemIterable, Condition, Info, PrintItemKind, Signal, PrintItem,
    ResolveConditionContext } from "@dprint/core";

export function* parseNode(node: Node) {
    // In a real implementation, this function would parse comments as well.

    switch (node.kind) {
        case SyntaxKind.ArrayLiteralExpression:
            yield* parseArrayLiteralExpression(expr);
            break;
        case SyntaxKind.ArrayElement:
            yield* parseArrayElement(expr);
            break;
    }
}

// node functions

function* parseArrayLiteralExpression(expr: ArrayLiteralExpression): PrintItemIterable {
    const startInfo = createInfo("startArrayExpression");
    const endInfo = createInfo("endArrayExpression");

    yield startInfo;

    yield "[";
    yield ifMultipleLines(Signal.NewLine);

    const elements = makeRepeatable(parseElements());
    yield {
        kind: PrintItemKind.Condition,
        name: "indentIfMultipleLines",
        condition: isMultipleLines,
        true: withIndent(elements),
        false: elements
    };

    yield ifMultipleLines(Signal.NewLine);
    yield "]";

    yield endInfo;

    function* parseElements(): PrintItemIterable {
        for (let i = 0; i < expr.elements.length; i++) {
            yield* parseNode(expr.elements[i]);

            if (i < expr.elements.length - 1) {
                yield ",";
                yield ifMultipleLines(Signal.NewLine, Signal.SpaceOrNewLine);
            }
        }
    }

    function ifMultipleLines(trueItem: PrintItem, falseItem?: PrintItem): Condition {
        return {
            kind: PrintItemKind.Condition,
            name: "ifMultipleLines",
            condition: isMultipleLines,
            true: [trueItem],
            false: falseItem == null ? undefined : [falseItem]
        };
    }

    // condition resolver
    function isMultipleLines(conditionContext: ResolveConditionContext) {
        // no elements, so format on the same line
        if (expr.elements.length === 0)
            return false;
        // first element is on a different line than the start of the array expression,
        // so format all the elements as multi-line
        if (expr.lineNumber < expr.elements[0].lineNumber)
            return true;
        // only one element, so force it to be a single line
        if (expr.elements.length === 1)
            return false;
        // check if the expression spans multiple lines, and if it does then make it multi-line
        const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo)!;
        const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);
        if (resolvedEndInfo == null)
            return undefined;
        return resolvedStartInfo.lineNumber < resolvedEndInfo.lineNumber;
    }
}

function* parseArrayElement(element: ArrayElement): PrintItemIterable {
    yield element.text;
}

// helper functions

function createInfo(name: string): Info {
    return { kind: PrintItemKind.Info, name };
}

function* withIndent(items: PrintItemIterable) {
    yield Signal.StartIndent;
    yield* items;
    yield Signal.FinishIndent;
}

function makeRepeatable(items: PrintItemIterable) {
    return Array.from(items);
}
```