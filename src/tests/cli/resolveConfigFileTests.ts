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

        expect(err.message).to.equal("Could not find configuration file at '/dprint.config'. Did you mean to create one or specify a --config option?\n\n"
            + "Error: File not found.");
    });

    it("should error when it can't find it when specifying a file", async () => {
        const environment = new TestEnvironment();
        const err = await getError("file.config", environment);

        expect(err.message).to.equal("Could not find specified configuration file at '/file.config'. Did you mean to create it?\n\n"
            + "Error: File not found.");
    });

    it("should get the default configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.config", `{ "semiColons": true }`);
        const config = await resolveConfigFile(undefined, environment);

        expect(config).to.deep.equal({ semiColons: true });
    });

    it("should get the specified configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/file.config", `{ "semiColons": true }`);
        const config = await resolveConfigFile("file.config", environment);

        expect(config).to.deep.equal({ semiColons: true });
    });

    it("should get the specified configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.addFile("/dprint.config", `{ semiColons: true }`);
        const err = await getError(undefined, environment);

        expect(err.message).to.equal("Error parsing configuration file (/dprint.config).\n\nSyntaxError: Unexpected token s in JSON at position 2");
    });
});
