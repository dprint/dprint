import { expect } from "chai";
import { print } from "../../printing";
import { PrintItemIterable, Condition, Info, PrintItemKind, Signal, PrintItem, ResolveConditionContext } from "../../types";

describe("parsing example", () => {
    // example AST nodes

    interface Node {
        /** Line number in the original source code. */
        lineNumber: number;
        /** Column number in the original source code. */
        columnNumber: number;
    }

    interface ArrayLiteralExpression extends Node {
        elements: ArrayElement[];
    }

    interface ArrayElement extends Node {
        text: string;
    }

    // IR generation

    function* parseArrayLiteralExpression(expr: ArrayLiteralExpression): PrintItemIterable {
        const startInfo = createInfo("startItems");
        const endInfo = createInfo("endItems");

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
                yield expr.elements[i].text;

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
            if (expr.elements.length === 0)
                return false;
            if (expr.lineNumber < expr.elements[0].lineNumber)
                return true;
            if (expr.elements.length === 1)
                return false;
            const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo)!;
            const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);
            if (resolvedEndInfo == null)
                return false;
            return resolvedStartInfo.lineNumber < resolvedEndInfo.lineNumber;
        }
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
            newlineKind: "\n",
            useTabs: false
        });

        expect(result).to.equal(expectedText);
    }

    it("should format when doesn't exceed line", () => {
        doTest({
            columnNumber: 0,
            lineNumber: 0,
            elements: [{
                columnNumber: 1,
                lineNumber: 0,
                text: "test"
            }, {
                columnNumber: 6,
                lineNumber: 0,
                text: "other"
            }]
        }, "[test, other]");
    });

    it("should format as multi-line when the first item is on a different line than the array expression", () => {
        doTest({
            columnNumber: 0,
            lineNumber: 0,
            elements: [{
                columnNumber: 1,
                lineNumber: 1,
                text: "test"
            }]
        }, "[\n  test\n]");
    });

    it("should format as single line when exceeding the print width with only one item", () => {
        const elementText = "asdfasdfasdfasdfasdfasdfasdfasdfasdfasdfasdfsadfasdf";
        doTest({
            columnNumber: 0,
            lineNumber: 0,
            elements: [{
                columnNumber: 1,
                lineNumber: 0,
                text: elementText
            }]
        }, `[${elementText}]`);
    });

    it("should format as multi-line when multiple items exceed the print width", () => {
        doTest({
            columnNumber: 0,
            lineNumber: 0,
            elements: [{
                columnNumber: 1,
                lineNumber: 0,
                text: "test"
            }, {
                columnNumber: 6,
                lineNumber: 0,
                text: "other"
            }, {
                columnNumber: 25,
                lineNumber: 0,
                text: "asdfasdfasdfasdfasdfasdfasdf"
            }]
        }, "[\n  test,\n  other,\n  asdfasdfasdfasdfasdfasdfasdf\n]");
    });
});
