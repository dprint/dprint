import { expect } from "chai";
import { handleCommandLineOptions, CommandLineOptions } from "../../cli";
import { getDefaultCommandLineOptions } from "./getDefaultCommandLineOptions";
import { TestEnvironment } from "./TestEnvironment";

describe(nameof(handleCommandLineOptions), () => {
    function handleOptions(options: Partial<CommandLineOptions>, environment: TestEnvironment) {
        handleCommandLineOptions({ ...getDefaultCommandLineOptions(), ...options }, environment);
    }

    function getLogs(options: Partial<CommandLineOptions>) {
        const environment = new TestEnvironment();
        handleOptions(options, environment);
        return environment.getLogs();
    }

    it("should output the help when specifying help", () => {
        const logs = getLogs({ showHelp: true });
        expect(logs.length).to.equal(1);
        expect(/^Version [0-9]+\.[0-9]+\.[0-9]+.*/.test(logs[0])).to.be.true;
    });

    it("should output the version when specifying version", () => {
        const logs = getLogs({ showVersion: true });
        expect(logs.length).to.equal(1);
        expect(/^[0-9]+\.[0-9]+\.[0-9]+.*$/.test(logs[0])).to.be.true;
    });
});
