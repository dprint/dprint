import { expect } from "chai";
import { Plugin } from "@dprint/types";
import { runCliWithOptions, CommandLineOptions, Configuration } from "../../cli";
import { getDefaultCommandLineOptions } from "./getDefaultCommandLineOptions";
import { TestEnvironment } from "./TestEnvironment";
import { TestPlugin } from "./TestPlugin";

describe(nameof(runCliWithOptions), () => {
    function createTestEnvironment(opts: { config?: { config: Configuration; }; } = {}) {
        const environment = new TestEnvironment();
        environment.setRequireObject("/dprint.config.js", opts.config || createConfig());
        environment.addFile("/file1.ts", "5+1;");
        environment.addFile("/file2.ts", "7+2");
        environment.addFile("/node_modules/otherFile.ts", ""); // should ignore this
        return environment;
    }

    function createConfig(opts: { plugins?: Plugin[]; includes?: string[]; excludes?: string[]; } = {}): { config: Configuration; } {
        return {
            config: {
                projectType: "openSource",
                newLineKind: "lf",
                plugins: opts.plugins || [new TestPlugin() as Plugin],
                includes: opts.includes,
                excludes: opts.excludes,
            },
        };
    }

    function handleOptions(options: Partial<CommandLineOptions>, environment: TestEnvironment) {
        return runCliWithOptions({ ...getDefaultCommandLineOptions(), ...options }, environment);
    }

    async function getLogs(options: Partial<CommandLineOptions>, environment = createTestEnvironment()) {
        await handleOptions(options, environment);
        return environment.getLogs();
    }

    async function getWarns(options: Partial<CommandLineOptions>, environment = createTestEnvironment()) {
        await handleOptions(options, environment);
        return environment.getWarns();
    }

    it("should output the help when specifying help", async () => {
        const logs = await getLogs({ showHelp: true });
        expect(logs).to.deep.equal([
            `dprint vPACKAGE_VERSION

Syntax:   dprint [options] [...file patterns]
Examples: dprint
          dprint "src/**/*.ts"
Options:
-h, --help              Outputs this message.
-v, --version           Outputs the version of the library and plugins.
--init                  Creates a dprint.config.js file in the current directory.
-c, --config            Configuration file to use (default: dprint.config.js)
--outputFilePaths       Outputs the list of file paths found for formatting without formatting the files.
--outputResolvedConfig  Outputs the resolved configuration from the configuration file.
--duration              Outputs how long the format took.
--allowNodeModuleFiles  Allows including files that have a node_modules directory in their path.
Plugins:
* dprint-plugin-test v0.1.0`,
        ]);
    });

    it("should output the help when there is no configuration file", async () => {
        const environment = createTestEnvironment();
        environment.removeRequireObject("/dprint.config.js");
        const logs = await getLogs({ showHelp: true }, environment);
        expect(logs.length).to.equal(1);
        expect(logs[0].endsWith("Plugins: [No plugins]")).to.be.true;
    });

    it("should output the version when specifying version", async () => {
        const logs = await getLogs({ showVersion: true });
        expect(logs).to.deep.equal([
            `dprint vPACKAGE_VERSION
dprint-plugin-test v0.1.0`,
        ]);
    });

    it("should output the version when there is no configuration file", async () => {
        const environment = createTestEnvironment();
        environment.removeRequireObject("/dprint.config.js");
        const logs = await getLogs({ showVersion: true }, environment);
        expect(logs).to.deep.equal([`dprint vPACKAGE_VERSION (No plugins)`]);
    });

    it("should initialize when providing --init and no config file exists", async () => {
        const environment = createTestEnvironment();
        const logs = await getLogs({ init: true }, environment);
        expect(logs.length).to.equal(1);
        expect(logs[0]).to.equal("Created /dprint.config.js");
        const fileText = await environment.readFile("/dprint.config.js");
        expect(fileText).to.equal(`// @ts-check
const { TypeScriptPlugin } = require("dprint-plugin-typescript");
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({
        }),
        new JsoncPlugin({
            indentWidth: 2
        })
    ],
    includes: [
        "**/*.{ts,tsx,json,js,jsx}"
    ]
};
`);
    });

    it("should warn when providing --init and a config file exists", async () => {
        const environment = createTestEnvironment();
        await environment.writeFile("/dprint.config.js", "test");
        const warns = await getWarns({ init: true }, environment);
        expect(warns.length).to.equal(1);
        expect(warns[0]).to.equal("Skipping initialization because a configuration file already exists at: /dprint.config.js");
        const fileText = await environment.readFile("/dprint.config.js");
        expect(fileText).to.equal("test");
    });

    it("should output the file paths when specifying to", async () => {
        const logs = await getLogs({ outputFilePaths: true, filePatterns: ["**/*.ts"] });
        expect(logs).to.deep.equal(["/file1.ts", "/file2.ts"]);
    });

    it("should output the file path count when there are none and outputting the file paths is specified", async () => {
        const logs = await getLogs({ outputFilePaths: true, filePatterns: ["asdfasf"] });
        expect(logs).to.deep.equal(["Found 0 files."]);
    });

    it("should not output the file paths when not specifying to", async () => {
        const logs = await getLogs({});
        expect(logs.length).to.equal(0);
    });

    it("should format the files", async () => {
        const environment = createTestEnvironment();
        await handleOptions({ filePatterns: ["**/*.ts"] }, environment);
        expect(await environment.readFile("/file1.ts")).to.equal("// formatted\n5+1;");
        expect(await environment.readFile("/file2.ts")).to.equal("// formatted\n7+2");
    });

    it("should log when a file can't be formatted", async () => {
        const environment = createTestEnvironment();
        environment.addFile("file.asdf", "test");
        await handleOptions({ filePatterns: ["**/*.asdf"] }, environment);
        expect(environment.getErrors()).to.deep.equal([
            "Error formatting file: file.asdf\n\nError: Could not find a plugin that would parse the file at path: file.asdf",
        ]);
    });

    it("should output the resolved config when specifying to", async () => {
        const logs = await getLogs({ outputResolvedConfig: true });
        expect(logs.length).to.equal(1);
        expect(logs[0].startsWith("Global configuration: {")).to.be.true;
    });

    it("should include node_module files when specifying to", async () => {
        const logs = await getLogs({ outputFilePaths: true, allowNodeModuleFiles: true, filePatterns: ["**/*.ts"] });
        expect(logs).to.deep.equal(["/file1.ts", "/file2.ts", "/node_modules/otherFile.ts"]);
    });

    const projectTypeMissingWarningText = `[dprint.config.js]: The "projectType" field is missing. You may specify any of the following possible values `
        + `in the configuration file according to your conscience and that will supress this warning.\n\n`
        + ` * openSource              Dprint is formatting an open source project.\n`
        + ` * commercialSponsored     Dprint is formatting a closed source commercial project and your company sponsored dprint.\n`
        + ` * commercialDidNotSponsor Dprint is formatting a closed source commercial project and you want to forever enshrine your name `
        + `in source control for having specified this.`;

    it("should warn when not specifying a project type field", async () => {
        const environment = createTestEnvironment();
        environment.setRequireObject("/dprint.config.js", {
            config: {
                newLineKind: "lf",
            },
        });
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(1);
        expect(warns[0]).to.equal(projectTypeMissingWarningText);
    });

    it("should warn when specifying an incorrect project type field", async () => {
        const environment = createTestEnvironment();
        environment.setRequireObject("/dprint.config.js", {
            config: {
                projectType: "asdf",
                newLineKind: "lf",
            },
        });
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(1);
        expect(warns[0]).to.equal(projectTypeMissingWarningText);
    });

    it("should not warn when specifying a correct project type field", async () => {
        const environment = createTestEnvironment();
        environment.setRequireObject("/dprint.config.js", {
            config: {
                projectType: "openSource",
                newLineKind: "lf",
            },
        });
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(0);
    });

    it("should output the duration when specifying to", async () => {
        const logs = await getLogs({ duration: true });
        expect(logs.length).to.equal(1);
        expect(/^Duration: [0-9]+\.[0-9][0-9]s$/.test(logs[0])).to.be.true;
    });

    it("should format the files specified in the includes", async () => {
        const config = createConfig({
            includes: ["/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ outputFilePaths: true }, environment);
        expect(logs).to.deep.equal(["/file1.ts"]);
    });

    it("should format the files specified in the includes and exclude the ones in the excludes", async () => {
        const config = createConfig({
            includes: ["**/*.ts"],
            excludes: ["/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ outputFilePaths: true }, environment);
        expect(logs).to.deep.equal(["/file2.ts"]);
        expect(environment.getWarns()).to.deep.equal([]);
    });

    it("should exclude the ones in the excludes even when already negated", async () => {
        const config = createConfig({
            includes: ["**/*.ts"],
            excludes: ["!/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ outputFilePaths: true }, environment);
        expect(logs).to.deep.equal(["/file2.ts"]);
        expect(environment.getWarns()).to.deep.equal([]);
    });

    it("should format the files specified in the command args and exclude the ones in the excludes", async () => {
        const config = createConfig({
            excludes: ["/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ outputFilePaths: true, filePatterns: ["**/*.ts"] }, environment);
        expect(logs).to.deep.equal(["/file2.ts"]);
        expect(environment.getWarns()).to.deep.equal([]);
    });

    it("should use file patterns when both cli args and includes are specified", async () => {
        const config = createConfig({
            includes: ["/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ outputFilePaths: true, filePatterns: ["/file2.ts"] }, environment);
        expect(logs).to.deep.equal(["/file2.ts"]);
        expect(environment.getWarns()).to.deep.equal([]);
    });

    it("should warn when both cli args and includes are specified", async () => {
        const config = createConfig({
            includes: ["/file1.ts"],
        });
        const environment = createTestEnvironment({ config });
        const logs = await getLogs({ filePatterns: ["/file2.ts"] }, environment);
        expect(logs).to.deep.equal([]);
        expect(environment.getWarns()).to.deep.equal([
            "Ignoring the configuration file's includes because file patterns were provided to the command line.",
        ]);
    });
});
