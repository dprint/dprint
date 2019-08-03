import { expect } from "chai";
import { removeStringIndentation } from "../../utils";

describe(nameof(removeStringIndentation), () => {
    function doTest(input: string, expectedOutput: string, options: { indentWidthInSpaces?: number; isInStringAtPos?: (pos: number) => boolean; } = {}) {
        const actualResult = removeStringIndentation(input, {
            indentWidthInSpaces: options.indentWidthInSpaces || 4,
            isInStringAtPos: options.isInStringAtPos || (() => false)
        });

        expect(actualResult).to.equal(expectedOutput);
    }

    it("should not do anything for a string on one line with no indentation", () => {
        doTest("testing", "testing");
    });

    it("should remove indentation on a single line", () => {
        doTest("    \t    testing", "testing");
    });

    it("should do nothing when one of the lines has no indentation, but others do", () => {
        const text = "testing\n    this\nout\n\tmore";
        doTest(text, text);
    });

    it("should remove hanging indentation", () => {
        doTest("testing\n    this", "testing\nthis");
    });

    it("should consider the first line's indent, but only if indented", () => {
        doTest("    testing\n        this", "testing\n    this");
    });

    it("should consider the first line's indent if only indented by one space and the tab size is 4", () => {
        doTest(" testing\n        this", "testing\n    this", { indentWidthInSpaces: 4 });
    });

    it("should consider the first line's indent if only indented by one space and the tab size is 2", () => {
        doTest(" testing\n    this", "testing\n  this", { indentWidthInSpaces: 2 });
    });

    it("should remove based on the minimum width", () => {
        doTest("{\n        test\n    }", "{\n    test\n}");
    });

    it("should remove tabs", () => {
        doTest("{\n\t\ttest\n\t}", "{\n\ttest\n}");
    });

    it("should treat tabs based on the tab size provided when mixing spaces and tabs", () => {
        doTest("{\n  \t  test\n    }", "{\n  test\n}", { indentWidthInSpaces: 2 });
    });

    it("should not deindent within strings", () => {
        let str = "this is a `";
        const pos = str.length;
        str += "\n    test`";
        const end = str.length;
        str += "\n    other";
        doTest(str, "this is a `\n    test`\nother", { isInStringAtPos: index => index >= pos && index < end });
    });
});
