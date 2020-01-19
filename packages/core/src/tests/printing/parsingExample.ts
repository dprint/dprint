import { expect } from "chai";
import { PrintItemIterable, Condition, Info, PrintItemKind, Signal, PrintItem, ResolveConditionContext } from "@dprint/types";
import { print } from "../../printer";

describe("parsing example", () => {
    // example AST nodes

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

    // IR generation

    function* parseNode(node: Node) {
        // this general function should parse comments

        switch (node.kind) {
            case SyntaxKind.ArrayLiteralExpression:
                yield* parseArrayLiteralExpression(node);
                break;
            case SyntaxKind.ArrayElement:
                yield* parseArrayElement(node);
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

    function doTest(expr: ArrayLiteralExpression, expectedText: string) {
        const printItems = parseArrayLiteralExpression(expr);
        const result = print(printItems, {
            indentWidth: 2,
            maxWidth: 40,
            newLineKind: "\n",
            useTabs: false,
            isTesting: true
        });

        expect(result).to.equal(expectedText);
    }

    it("should format when doesn't exceed line", () => {
        doTest({
            kind: SyntaxKind.ArrayLiteralExpression,
            lineNumber: 0,
            columnNumber: 0,
            elements: [{
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 1,
                text: "test"
            }, {
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 6,
                text: "other"
            }]
        }, "[test, other]");
    });

    it("should format as multi-line when the first item is on a different line than the array expression", () => {
        doTest({
            kind: SyntaxKind.ArrayLiteralExpression,
            lineNumber: 0,
            columnNumber: 0,
            elements: [{
                kind: SyntaxKind.ArrayElement,
                lineNumber: 1,
                columnNumber: 1,
                text: "test"
            }]
        }, "[\n  test\n]");
    });

    it("should format as single line when exceeding the print width with only one item", () => {
        const elementText = "asdfasdfasdfasdfasdfasdfasdfasdfasdfasdfasdfsadfasdf";
        doTest({
            kind: SyntaxKind.ArrayLiteralExpression,
            lineNumber: 0,
            columnNumber: 0,
            elements: [{
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 1,
                text: elementText
            }]
        }, `[${elementText}]`);
    });

    it("should format as multi-line when multiple items exceed the print width", () => {
        doTest({
            kind: SyntaxKind.ArrayLiteralExpression,
            lineNumber: 0,
            columnNumber: 0,
            elements: [{
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 1,
                text: "test"
            }, {
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 6,
                text: "other"
            }, {
                kind: SyntaxKind.ArrayElement,
                lineNumber: 0,
                columnNumber: 25,
                text: "asdfasdfasdfasdfasdfasdfasdf"
            }]
        }, "[\n  test,\n  other,\n  asdfasdfasdfasdfasdfasdfasdf\n]");
    });
});
