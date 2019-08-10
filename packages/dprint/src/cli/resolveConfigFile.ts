import { Configuration } from "@dprint/core";
import { Environment } from "../environment";
import { getError } from "../utils";

export interface ResolveConfigFileResult {
    /** Resolved file path of the configuration file. */
    filePath: string;
    /** Configuration specified in the file. */
    config: Configuration;
}

export async function resolveConfigFile(filePath: string | undefined, environment: Environment): Promise<ResolveConfigFileResult> {
    const resolvedFilePath = resolveConfigFilePath(filePath, environment);

    return {
        filePath: resolvedFilePath,
        config: await getConfig()
    };

    async function getConfig() {
        // todo: use a dynamic import here? (that's why this is currently using a promise)
        return new Promise<Configuration>((resolve, reject) => {
            try {
                const config = require(resolvedFilePath);
                if (typeof config !== "object" || typeof config.default !== "object")
                    reject(getError(`Expected an object being exported as the default export of the configuration at ${resolvedFilePath}`));
                else
                    resolve(config.default);
            } catch (err) {
                environment.exists(resolvedFilePath).then(exists => {
                    if (exists)
                        reject(getError(`Error loading configuration file '${resolvedFilePath}'.\n\n${err}`));
                    else if (filePath == null) {
                        reject(getError(
                            `Could not find configuration file at '${resolvedFilePath}'. `
                                + `Did you mean to create one or specify a --config option?\n\n`
                                + err
                        ));
                    }
                    else {
                        reject(getError(`Could not find specified configuration file at '${resolvedFilePath}'. Did you mean to create it?\n\n` + err));
                    }
                });
            }
        });
    }
}

function resolveConfigFilePath(filePath: string | undefined, environment: Environment) {
    return environment.resolvePath(filePath || "dprint.config.js");
}
