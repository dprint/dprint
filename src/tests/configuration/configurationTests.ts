import * as os from "os";
import { expect } from "chai";
import { formatFileText } from "../../formatFileText";

describe("new line configuration", () => {
    it("should use the first line newline kind when auto and using \n", () => {
        const result = formatFileText("file.ts", "var t;\nvar u;\r\n", { newLineKind: "auto" });
        expect(result).to.equal("var t;\nvar u;\n");
    });

    it("should use the first line newline kind when auto and using \r\n", () => {
        const result = formatFileText("file.ts", "var t;\r\nvar u;\n", { newLineKind: "auto" });
        expect(result).to.equal("var t;\r\nvar u;\r\n");
    });

    it("should use the system newline when set to system", () => {
        const result = formatFileText("file.ts", "var t;\r\nvar u;\n", { newLineKind: "system" });
        const newLine = os.EOL === "\r\n" ? "\r\n" : "\n";
        expect(result).to.equal(`var t;${newLine}var u;${newLine}`);
    });
});

describe("indent size configuration", () => {
    it("should format with the specified indent size", () => {
        const result = formatFileText("file.ts", "if (true) {\n      //1\nconsole.log(5); }", { indentSize: 2 });
        expect(result).to.equal("if (true) {\n  // 1\n  console.log(5);\n}\n");
    });
});

describe("use tabs configuration", () => {
    it("should use tabs when specified", () => {
        const result = formatFileText("file.ts", "if (true) {\n//1\nconsole.log(5); }", { useTabs: true });
        expect(result).to.equal("if (true) {\n\t// 1\n\tconsole.log(5);\n}\n");
    });
});
