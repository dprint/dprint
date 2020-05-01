import { expect } from "chai";
import { parseCommandLineArgs, CommandLineOptions } from "../../cli";
import { getDefaultCommandLineOptions } from "./getDefaultCommandLineOptions";

describe(nameof(parseCommandLineArgs), () => {
    function doTest(args: string[], expected: Partial<CommandLineOptions>) {
        const expectedOptions = { ...getDefaultCommandLineOptions(), ...expected };
        const options = parseCommandLineArgs(args);
        expect(options).to.deep.equal(expectedOptions);
    }

    it("should parse the default options", () => {
        doTest([], {});
    });

    it("should parse the -h option", () => {
        doTest(["-h"], { showHelp: true });
    });

    it("should parse the --help option", () => {
        doTest(["--help"], { showHelp: true });
    });

    it("should parse the -v option", () => {
        doTest(["-v"], { showVersion: true });
    });

    it("should parse the --version option", () => {
        doTest(["--version"], { showVersion: true });
    });

    it("should parse the -c option", () => {
        doTest(["-c", "file.config"], { config: "file.config" });
    });

    it("should parse the --config option", () => {
        doTest(["--config", "file.config"], { config: "file.config" });
    });

    it("should parse file globs specified with a leading config", () => {
        doTest(["--config", "file.config", "file.ts", "file2.ts"], { config: "file.config", filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading init", () => {
        doTest(["--init", "file.ts", "file2.ts"], { init: true, filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading help", () => {
        doTest(["--help", "file.ts", "file2.ts"], { showHelp: true, filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading version", () => {
        doTest(["--version", "file.ts", "file2.ts"], { showVersion: true, filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading outputFilePaths", () => {
        doTest(["--outputFilePaths", "file.ts", "file2.ts"], { outputFilePaths: true, filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading outputResolvedConfig", () => {
        doTest(["--outputResolvedConfig", "file.ts", "file2.ts"], { outputResolvedConfig: true, filePatterns: ["file.ts", "file2.ts"] });
    });

    it("should parse file globs specified with a leading allowNodeModuleFiles", () => {
        doTest(["--allowNodeModuleFiles", "file.ts", "file2.ts"], { allowNodeModuleFiles: true, filePatterns: ["file.ts", "file2.ts"] });
    });
});
