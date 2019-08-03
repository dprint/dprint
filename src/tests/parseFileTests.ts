import { expect } from "chai";
import { resolveConfiguration } from "../configuration";
import { getFileKind } from "../getFileKind";
import { FileKind } from "../FileKind";

describe(nameof(getFileKind), () => {
    const config = resolveConfiguration({}).config;

    function doSuccessTest(filePath: string, fileKind: FileKind) {
        expect(getFileKind(filePath)).to.equal(fileKind);
    }

    it("should get for a .ts file", () => {
        doSuccessTest("test.ts", FileKind.TypeScript);
    });

    it("should get for .tsx file", () => {
        doSuccessTest("test.tsx", FileKind.TypeScriptTsx);
    });

    it("should get for a .js file", () => {
        doSuccessTest("test.js", FileKind.TypeScript);
    });

    it("should get for a .jsx file", () => {
        doSuccessTest("test.jsx", FileKind.TypeScriptTsx);
    });

    it("should get for a .json file", () => {
        doSuccessTest("test.json", FileKind.Json);
    });

    it("shoudld throw when providing an unknown file extension", () => {
        expect(() => getFileKind("/src/test.txt")).to.throw("[dprint]: Could not resolve file kind based on file path: /src/test.txt");
    });
});
