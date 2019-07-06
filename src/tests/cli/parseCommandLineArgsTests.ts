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
});
