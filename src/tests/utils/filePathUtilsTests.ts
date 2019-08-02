import { expect } from "chai";
import { getFileExtension } from "../../utils";

describe(nameof(getFileExtension), () => {
    it("should get the file extension with period", () => {
        expect(getFileExtension("/test/usr/index.ts")).to.equal(".ts");
    });

    it("should return an empty string when there is no extension", () => {
        expect(getFileExtension("/test/usr/index")).to.equal("");
    });
});
