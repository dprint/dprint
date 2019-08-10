import { parse, ParseError } from "jsonc-parser";
import { GlobalConfiguration, Plugin } from "@dprint/core";
import { Environment } from "../environment";
import { throwError, formatJsonParserDiagnostics } from "../utils";

export interface ResolveConfigFileResult {
    /** Resolved file path of the configuration file. */
    filePath: string;
    /** Configuration specified in the file. */
    config: GlobalConfiguration & {
        plugins?: string[];
        [propertyName: string]: unknown;
    };
}

export async function resolveConfigFile(filePath: string | undefined, environment: Environment): Promise<ResolveConfigFileResult> {
    const resolvedFilePath = resolveConfigFilePath(filePath, environment);
    const fileText = await getFileText();

    const diagnostics: ParseError[] = [];
    const config = parse(fileText, diagnostics);

    if (diagnostics.length > 0)
        return throwError(`Error parsing configuration file (${resolvedFilePath}).\n\n` + formatJsonParserDiagnostics(diagnostics, fileText));

    return {
        filePath: resolvedFilePath,
        config
    };

    async function getFileText() {
        try {
            return await environment.readFile(resolvedFilePath);
        } catch (err) {
            if (filePath == null) {
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
    }
}

function resolveConfigFilePath(filePath: string | undefined, environment: Environment) {
    return environment.resolvePath(filePath || "dprint.json");
}
