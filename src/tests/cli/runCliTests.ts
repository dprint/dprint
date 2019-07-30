import { expect } from "chai";
import { runCliWithOptions, CommandLineOptions } from "../../cli";
import { getDefaultCommandLineOptions } from "./getDefaultCommandLineOptions";
import { TestEnvironment } from "./TestEnvironment";

describe(nameof(runCliWithOptions), () => {
    function createTestEnvironment() {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.config", `{ "newlineKind": "lf" }`);
        environment.addFile("/file1.ts", "5+1;");
        environment.addFile("/file2.ts", "console.log (5)  ;");
        environment.addFile("/node_modules/otherFile.ts", ""); // should ignore this
        return environment;
    }
    function handleOptions(options: Partial<CommandLineOptions>, environment: TestEnvironment) {
        return runCliWithOptions({ ...getDefaultCommandLineOptions(), ...options }, environment);
    }

    async function getLogs(options: Partial<CommandLineOptions>) {
        const environment = createTestEnvironment();
        await handleOptions(options, environment);
        return environment.getLogs();
    }

    it("should output the help when specifying help", async () => {
        const logs = await getLogs({ showHelp: true });
        expect(logs.length).to.equal(1);
        expect(/^Version [0-9]+\.[0-9]+\.[0-9]+.*/.test(logs[0])).to.be.true;
    });

    it("should output the version when specifying version", async () => {
        const logs = await getLogs({ showVersion: true });
        expect(logs.length).to.equal(1);
        expect(/^[0-9]+\.[0-9]+\.[0-9]+.*$/.test(logs[0])).to.be.true;
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
});
