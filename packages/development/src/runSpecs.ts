import { expect } from "chai";
import * as fs from "fs";
import * as path from "path";
import globby from "globby";
import { resolveConfiguration, formatFileText, CliLoggingEnvironment } from "@dprint/core";
import { Plugin, ConfigurationDiagnostic } from "@dprint/types";
import { getPrintIterableAsFormattedText } from "./getPrintIterableAsFormattedText";
import { parseSpecs, Spec } from "./specParser";

export interface RunSpecsOptions {
    specsDir: string;
    createPlugin: (config: unknown) => Plugin;
    defaultFileName: string;
}

export function runSpecs(options: RunSpecsOptions) {
    const { createPlugin, defaultFileName } = options;
    const specsDir = path.resolve(options.specsDir).replace(/\\/g, "/");
    const environment = new CliLoggingEnvironment();

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
            const globalConfig = getGlobalConfiguration();
            const plugin = getPlugin();

            const printIterable = plugin.parseFile(spec.filePath, spec.fileText);
            if (spec.showTree) {
                if (printIterable === false)
                    throw new Error("Can't print the tree because this file says it shouldn't be parsed.");
                console.log(getPrintIterableAsFormattedText(printIterable));
            }

            const actualText = formatFileText({
                filePath: spec.filePath,
                fileText: spec.fileText,
                plugins: [plugin]
            });

            if (!spec.expectedText.endsWith("\n"))
                throw new Error(`${spec.message}: The expected text did not end with a newline.`);
            if (spec.expectedText.endsWith("\n\n"))
                throw new Error(`${spec.message}: The expected text ended with multiple newlines: ${JSON.stringify(spec.expectedText)}`);
            // expect(JSON.stringify(actualText)).to.equal(JSON.stringify(spec.expectedText), spec.message);
            expect(actualText).to.equal(spec.expectedText, spec.message);

            function getGlobalConfiguration() {
                const result = resolveConfiguration({});
                throwIfConfigDiagnostics(result.diagnostics);
                return result.config;
            }

            function getPlugin() {
                const plugin = createPlugin(spec.config);
                plugin.initialize({
                    globalConfig,
                    environment
                });
                throwIfConfigDiagnostics(plugin.getConfigurationDiagnostics());
                return plugin;
            }

            function throwIfConfigDiagnostics(diagnostics: ConfigurationDiagnostic[]) {
                if (diagnostics.length > 0)
                    throw new Error(JSON.stringify(diagnostics));
            }
        }

        function getSpecs(filePath: string) {
            const fileText = readFileSync(filePath);
            try {
                return parseSpecs(fileText, { defaultFileName });
            } catch (err) {
                throw new Error(`Error parsing ${filePath}\n\n${err}`);
            }
        }
    });
}

function readFileSync(filePath: string) {
    return fs.readFileSync(filePath, { encoding: "utf8" });
}
