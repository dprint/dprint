import { expect } from "chai";
import { resolveConfigFile } from "../../cli";
import { TestEnvironment } from "./TestEnvironment";

describe(nameof(resolveConfigFile), () => {
    async function getError(filePath: string | undefined, environment: TestEnvironment) {
        let foundErr: { message: string; } | undefined;
        try {
            await resolveConfigFile(filePath, environment);
        } catch (err) {
            foundErr = err;
        }

        if (foundErr == null)
            expect.fail("Expected to have an error message.");

        return foundErr!;
    }

    it("should error when it can't find it when not specifying a file", async () => {
        const environment = new TestEnvironment();
        const err = await getError(undefined, environment);

        expect(err.message).to.equal(
            "[dprint]: Could not find configuration file at '/dprint.json'. "
                + "Did you mean to create one or specify a --config option?\n\n"
                + "Error: File not found."
        );
    });

    it("should error when it can't find it when specifying a file", async () => {
        const environment = new TestEnvironment();
        const err = await getError("file.config", environment);

        expect(err.message).to.equal(
            "[dprint]: Could not find specified configuration file at '/file.config'. "
                + "Did you mean to create it?\n\n"
                + "Error: File not found."
        );
    });

    it("should get the default configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.json", `{ "semiColons": true }`);
        const config = await resolveConfigFile(undefined, environment);

        expect(config.filePath).to.equal("/dprint.json");
        expect(config.config).to.deep.equal({ semiColons: true });
    });

    it("should get the specified configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/file.config", `{ "semiColons": true }`);
        const config = await resolveConfigFile("file.config", environment);

        expect(config.filePath).to.equal("/file.config");
        expect(config.config).to.deep.equal({ semiColons: true });
    });

    it("should get the specified configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.json", `{ semiColons: true }`);
        const err = await getError(undefined, environment);

        expect(err.message).to.equal(
            "[dprint]: Error parsing configuration file (/dprint.json).\n\n"
                + "InvalidSymbol: semiColons (2)\n"
                + "PropertyNameExpected: : (12)\n"
                + "ValueExpected: } (19)"
        );
    });

    it("should get the configuration file when it has comments", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.json", `{\n  // testing\n  /* testing */\n  "semiColons": true\n}\n`);
        const config = await resolveConfigFile(undefined, environment);

        expect(config.config).to.deep.equal({ semiColons: true });
    });
});
