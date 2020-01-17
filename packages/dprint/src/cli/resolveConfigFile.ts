import { Configuration as CoreConfiguration, WebAssemblyPlugin } from "@dprint/types";
import { Environment } from "../environment";
import { throwError } from "../utils";

/** Configuration for a dprint.config.js file. */
export interface Configuration extends CoreConfiguration {
    /** File globs of files to format. */
    includes?: string[];
    /** File globs of files to not format. */
    excludes?: string[];
}

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
            try {
                config = await environment.require(resolvedFilePath);
            } catch (err) {
                if (await environment.exists(resolvedFilePath))
                    return throwError(`Error loading configuration file '${resolvedFilePath}'.\n\n${err}`);
                else if (filePath == null) {
                    return throwError(
                        `Could not find configuration file at '${resolvedFilePath}'. `
                            + `Did you mean to create one (dprint --init) or specify a --config option?\n\n`
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
        } catch (err) {
            // dispose the plugins on error if they were created
            const plugins = (config as ResolveConfigFileResult)?.config?.plugins;
            if (plugins instanceof Array)
                plugins.forEach(p => (p as WebAssemblyPlugin)?.dispose?.());

            throw err;
        }
    }
}

export function resolveConfigFilePath(filePath: string | undefined, environment: Environment) {
    return environment.resolvePath(filePath || "dprint.config.js");
}
