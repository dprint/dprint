import { expect } from "chai";
import { runCliWithOptions, CommandLineOptions } from "../../cli";
import { getDefaultCommandLineOptions } from "./getDefaultCommandLineOptions";
import { TestEnvironment } from "./TestEnvironment";

describe(nameof(runCliWithOptions), () => {
    function createTestEnvironment() {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.json", `{ "projectType": "openSource", "newlineKind": "lf" }`);
        environment.addFile("/file1.ts", "5+1;");
        environment.addFile("/file2.ts", "console.log (5)  ;");
        environment.addFile("/node_modules/otherFile.ts", ""); // should ignore this
        return environment;
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
        expect(logs.length).to.equal(1);
        expect(logs[0].startsWith("Version PACKAGE_VERSION\nSyntax:")).to.be.true;
    });

    it("should output the version when specifying version", async () => {
        const logs = await getLogs({ showVersion: true });
        expect(logs.length).to.equal(1);
        expect(logs[0]).to.equal("PACKAGE_VERSION");
    });

    it("should output the file paths when specifying to", async () => {
        const logs = await getLogs({ outputFilePaths: true });
        expect(logs).to.deep.equal(["/file1.ts", "/file2.ts"]);
    });

    it("should not output the file paths when not specifying to", async () => {
        const logs = await getLogs({});
        expect(logs.length).to.equal(0);
    });

    it("should format the files", async () => {
        const environment = createTestEnvironment();
        await handleOptions({}, environment);
        expect(await environment.readFile("/file1.ts")).to.equal("5 + 1;\n");
        expect(await environment.readFile("/file2.ts")).to.equal("console.log(5);\n");
    });

    it("should output the resolved config when specifying to", async () => {
        const logs = await getLogs({ outputResolvedConfig: true });
        expect(logs.length).to.equal(1);
        expect(logs[0].startsWith("{")).to.be.true;
    });

    it("should include node_module files when specifying to", async () => {
        const logs = await getLogs({ outputFilePaths: true, allowNodeModuleFiles: true });
        expect(logs).to.deep.equal(["/file1.ts", "/file2.ts", "/node_modules/otherFile.ts"]);
    });

    const projectTypeMissingWarningText = `[dprint.json]: The "projectType" field is missing. You may specify any of the following possible values `
        + `in the configuration file according to your conscience and that will supress this warning.\n\n`
        + ` * openSource              Dprint is formatting an open source project.\n`
        + ` * commercialSponsored     Dprint is formatting a closed source commercial project and your company sponsored dprint.\n`
        + ` * commercialDidNotSponsor Dprint is formatting a closed source commercial project and you want to forever enshrine your name `
        + `in source control for having specified this.`;

    it("should warn when not specifying a project type field", async () => {
        const environment = createTestEnvironment();
        environment.addFile("/dprint.json", `{ "newlineKind": "lf" }`);
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(1);
        expect(warns[0]).to.equal(projectTypeMissingWarningText);
    });

    it("should warn when specifying an incorrect project type field", async () => {
        const environment = createTestEnvironment();
        environment.addFile("/dprint.json", `{ "projectType": "asdf", "newlineKind": "lf" }`);
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(1);
        expect(warns[0]).to.equal(projectTypeMissingWarningText);
    });

    it("should not warn when specifying a correct project type field", async () => {
        const environment = createTestEnvironment();
        environment.addFile("/dprint.json", `{ "projectType": "openSource", "newlineKind": "lf" }`);
        const warns = await getWarns({ filePatterns: ["**/*.ts"] }, environment);
        expect(warns.length).to.equal(0);
    });

    it("should output the duration when specifying to", async () => {
        const logs = await getLogs({ duration: true });
        expect(logs.length).to.equal(1);
        expect(/^Duration: [0-9]+\.[0-9][0-9]s$/.test(logs[0])).to.be.true;
    });
});
