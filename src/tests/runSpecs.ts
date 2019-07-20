import { expect } from "chai";
import * as fs from "fs";
import * as path from "path";
import globby from "globby";
import { formatFileText } from "../formatFileText";
import { parseSpecs, Spec } from "./specParser";
import { resolveConfiguration } from "../configuration";
import { parseToBabelAst, parseFile } from "../parsing";
import { printTree } from "./printTree";

const rootDir = path.join(__dirname, "../../");
const specsDir = path.resolve(path.join(rootDir, "src/tests/specs")).replace(/\\/g, "/");

describe("specs", () => {
    // blocking here for mocha. todo: figure out how to load test cases asynchronously
    const filePaths = globby.sync(`${specsDir}/**/*.txt`);
    const onlyFilePaths = filePaths.filter(filePath => filePath.toLowerCase().endsWith("_only.txt"));

    if (onlyFilePaths.length > 0) {
        filePaths.length = 0;
        filePaths.push(...onlyFilePaths);
    }

    for (const filePath of filePaths) {
        describe(path.basename(filePath), () => {
            const specs = getSpecs(filePath);
            for (const spec of specs) {
                const itFunc = spec.isOnly ? it.only : it;

                if (spec.isOnly)
                    console.log("RUNNING ONLY TEST!");

                itFunc(spec.message, () => {
                    runTest(spec);
                });
            }
        });
    }

    function runTest(spec: Spec) {
        const resolvedConfig = resolveConfiguration(spec.config);
        if (resolvedConfig.diagnostics.length > 0)
            throw new Error(JSON.stringify(resolvedConfig.diagnostics));

        if (spec.showTree) {
            const babelAst = parseToBabelAst(spec.filePath, spec.fileText);
            const printItem = parseFile(babelAst, spec.fileText, resolvedConfig.config);
            console.log(printTree(printItem));
        }

        const actualText = formatFileText(spec.filePath, spec.fileText, resolvedConfig.config);
        if (!spec.expectedText.endsWith("\n"))
            throw new Error(`${spec.message}: The expected text did not end with a newline.`);
        if (spec.expectedText.endsWith("\n\n"))
            throw new Error(`${spec.message}: The expected text ended with multiple newlines: ${JSON.stringify(spec.expectedText)}`);
        //expect(JSON.stringify(actualText)).to.equal(JSON.stringify(spec.expectedText), spec.message);
        expect(actualText).to.equal(spec.expectedText, spec.message);
    }

    function getSpecs(filePath: string) {
        const fileText = readFileSync(filePath);
        try {
            return parseSpecs(fileText);
        } catch (err) {
            throw new Error(`Error parsing ${filePath}\n\n${err}`);
        }
    }
});

function readFileSync(filePath: string) {
    return fs.readFileSync(filePath, { encoding: "utf8" });
}

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
