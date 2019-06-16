import { expect } from "chai";
import * as fs from "fs";
import * as path from "path";
import globby from "globby";
import { formatFileText } from "../formatFileText";

const rootDir = path.join(__dirname, "../../");
const specsDir = path.resolve(path.join(rootDir, "src/tests/specs"))

describe.only("specs", () => {
    // blocking here for mocha (not sure if it works async) todo: figure out if it works async
    const filePaths = globby.sync(`${specsDir}/**/*.txt`);
    const onlyFilePaths = filePaths.filter(filePath => filePath.toLowerCase().endsWith("_only.txt"));

    if (onlyFilePaths.length > 0) {
        filePaths.length = 0;
        filePaths.push(...onlyFilePaths);
    }

    for (const filePath of filePaths) {
        it(`should work for ${path.basename(filePath)}`, async () => {
            await runTest(filePath);
        });
    }

    async function runTest(filePath: string) {
        const fileText = await readFile(filePath);
        const parts = fileText.split("[expect]").map(text => text.replace(/\r?\n/g, "\n"));
        const startText = parts[0].substring(0, parts[0].length - 1); // remove last newline
        const expectedText = parts[1].substring(1, parts[1].length); // remove first newline
        const actualText = formatFileText(filePath, startText);

        expect(actualText).to.equal(expectedText);
    }
});

function readFile(filePath: string) {
    return new Promise<string>((resolve, reject) => {
        fs.readFile(filePath, { encoding: "utf8" }, (err, text) => {
            if (err)
                reject(err)
            else
                resolve(text);
        });
    });
}
