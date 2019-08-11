import { Configuration } from "@dprint/core";
import { Environment } from "../environment";
import { throwError } from "../utils";

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
        let config: any;
        try {
            config = await environment.require(resolvedFilePath);
        } catch (err) {
            if (await environment.exists(resolvedFilePath))
                return throwError(`Error loading configuration file '${resolvedFilePath}'.\n\n${err}`);
            else if (filePath == null) {
                return throwError(
                    `Could not find configuration file at '${resolvedFilePath}'. `
                        + `Did you mean to create one or specify a --config option?\n\n`
                        + err
                );
            }
            else {
                return throwError(`Could not find specified configuration file at '${resolvedFilePath}'. Did you mean to create it?\n\n` + err);
            }
        }

        if (typeof config !== "object" || typeof config.config !== "object")
            return throwError(`Expected an object to be exported on the 'config' named export of the configuration at '${resolvedFilePath}'.`);
        else
            return config.config;
    }
}

function resolveConfigFilePath(filePath: string | undefined, environment: Environment) {
    return environment.resolvePath(filePath || "dprint.config.js");
}
