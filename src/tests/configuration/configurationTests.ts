import * as os from "os";
import { expect } from "chai";
import { formatFileText } from "../../formatFileText";
import { resolveConfiguration, Configuration } from "../../configuration";

function doTest(inputText: string, config: Configuration, expectedText: string) {
    const configResult = resolveConfiguration(config);
    if (configResult.diagnostics.length > 0)
        throw configResult.diagnostics;

    const result = formatFileText("file.ts", inputText, configResult.config);
    expect(result).to.equal(expectedText);
}

describe("new line configuration", () => {
    it("should use the last line newline kind when auto and using \r\n", () => {
        doTest("var t;\nvar u;\r\n", { newLineKind: "auto" }, "var t;\r\nvar u;\r\n");
    });

    it("should use the first last newline kind when auto and using \n", () => {
        doTest("var t;\r\nvar u;\n", { newLineKind: "auto" }, "var t;\nvar u;\n");
    });

    it("should use the system newline when set to system", () => {
        const newLine = os.EOL === "\r\n" ? "\r\n" : "\n";
        doTest("var t;\r\nvar u;\n", { newLineKind: "system" }, `var t;${newLine}var u;${newLine}`);
    });
});

describe("indent size configuration", () => {
    it("should format with the specified indent size", () => {
        doTest("if (true) {\n      //1\nconsole.log(5); }", { indentSize: 2 }, "if (true) {\n  // 1\n  console.log(5);\n}\n");
    });
});

describe("use tabs configuration", () => {
    it("should use tabs when specified", () => {
        doTest("if (true) {\n//1\nconsole.log(5); }", { useTabs: true }, "if (true) {\n\t// 1\n\tconsole.log(5);\n}\n");
    });
});
