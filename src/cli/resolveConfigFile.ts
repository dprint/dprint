import { Environment } from "../environment";
import { Configuration } from "../configuration";

const defaultFileName = "dprint.config";

export async function resolveConfigFile(filePath: string | undefined, environment: Environment): Promise<Configuration> {
    const resolvedFilePath = environment.resolvePath(filePath || defaultFileName);
    const fileText = await getFileText();

    try {
        return JSON.parse(fileText) as Configuration;
    } catch (err) {
        throw new Error(`Error parsing configuration file (${resolvedFilePath}).\n\n` + err);
    }

    async function getFileText() {
        try {
            return await environment.readFile(resolvedFilePath)
        } catch (err) {
            if (filePath == null)
                throw new Error(`Could not find configuration file at '${resolvedFilePath}'. Did you mean to create one or specify a --config option?\n\n` + err);
            else
                throw new Error(`Could not find specified configuration file at '${resolvedFilePath}'. Did you mean to create it?\n\n` + err);
        }
    }
}
