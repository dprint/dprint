import { expect } from "chai";
import { resolveConfiguration } from "../../configuration";
import { parseFile } from "../../parsing";

describe(nameof(parseFile), () => {
    const config = resolveConfiguration({}).config;

    function doSuccessTest(filePath: string, fileText: string) {
        expect(parseFile(filePath, fileText, config)).to.not.be.null.and.not.be.undefined;
    }

    it("should parse a .ts file", () => {
        doSuccessTest("test.ts", "const t = 5");
    });

    it("should parse a .tsx file", () => {
        doSuccessTest("test.tsx", "const t = <div />;");
    });

    it("should parse a .js file", () => {
        doSuccessTest("test.ts", "const t = 5");
    });

    it("should parse a .jsx file", () => {
        doSuccessTest("test.jsx", "const t = <div />;");
    });

    it("should parse a .json file", () => {
        doSuccessTest("test.json", "{}");
    });

    it("shoudld throw when providing an unknown file extension", () => {
        expect(() => parseFile("/src/test.txt", "", config)).to.throw("[dprint]: Could not resolve parser based on file path: /src/test.txt");
    });
});
