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
            "[dprint]: Could not find configuration file at '/dprint.config.js'. "
                + "Did you mean to create one (dprint --init) or specify a --config option?\n\n"
                + "Error: File not found."
        );
    });

    it("should error when it can't find it when specifying a file", async () => {
        const environment = new TestEnvironment();
        const err = await getError("file.test.js", environment);

        expect(err.message).to.equal(
            "[dprint]: Could not find specified configuration file at '/file.test.js'. "
                + "Did you mean to create it?\n\n"
                + "Error: File not found."
        );
    });

    it("should get the default configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.setRequireObject("/dprint.config.js", {
            config: {
                semiColons: true
            }
        });
        const config = await resolveConfigFile(undefined, environment);

        expect(config.filePath).to.equal("/dprint.config.js");
        expect(config.config).to.deep.equal({ semiColons: true });
    });

    it("should get the specified configuration file when it exists", async () => {
        const environment = new TestEnvironment();
        environment.setRequireObject("/file.js", {
            config: {
                semiColons: true
            }
        });
        const config = await resolveConfigFile("file.js", environment);

        expect(config.filePath).to.equal("/file.js");
        expect(config.config).to.deep.equal({ semiColons: true });
    });

    it("should throw when there is no config object", async () => {
        const environment = new TestEnvironment();
        environment.setRequireObject("/file.js", {});
        const err = await getError("file.js", environment);

        expect(err.message).to.equal(
            "[dprint]: Expected an object to be exported on the 'config' named export of the configuration at '/file.js'."
        );
    });
});
