import { Environment } from "../environment";
import { formatFileText } from "../formatFileText";
import { parseCommandLineArgs } from "./parseCommandLineArgs";
import { getHelpText } from "./getHelpText";
import { getPackageVersion } from "./getPackageVersion";
import { CommandLineOptions } from "./CommandLineOptions";
import { resolveConfigFile } from "./resolveConfigFile";
import { resolveConfiguration } from "../configuration";

/**
 * Function used by the cli to format files.
 * @param args - Command line arguments.
 * @param environment - Environment to run the cli in.
 */
export async function runCli(args: string[], environment: Environment) {
    const options = parseCommandLineArgs(args);
    await runCliWithOptions(options, environment);
}

export async function runCliWithOptions(options: CommandLineOptions, environment: Environment) {
    if (options.showHelp) {
        environment.log(getHelpText());
        return;
    }
    else if (options.showVersion) {
        environment.log(getPackageVersion());
        return;
    }

    const unresolvedConfiguration = await resolveConfigFile(options.config, environment);
    const configResult = resolveConfiguration(unresolvedConfiguration);
    const { config } = configResult;

    for (const diagnostic of configResult.diagnostics)
        environment.warn(diagnostic.message);

    const filePaths = await getFilePaths();

    if (options.outputFilePaths) {
        for (const filePath of filePaths)
            environment.log(filePath);
        return;
    }
    else if (options.outputResolvedConfig) {
        // todo: print this out formatted
        environment.log(JSON.stringify(configResult.config));
        return;
    }

    const promises: Promise<void>[] = [];

    for (const filePath of filePaths) {
        const promise = environment.readFile(filePath).then(fileText => {
            const result = formatFileText(filePath, fileText, config);
            return environment.writeFile(filePath, result);
        }).catch(err => {
            environment.error(`Error formatting file: ${filePath}\n\n${err}`);
        });
        promises.push(promise);
    }

    return Promise.all(promises);

    async function getFilePaths() {
        const isInNodeModules = /[\/|\\]node_modules[\/|\\]/i;
        const allFilePaths = await environment.glob(options.filePatterns);

        return options.allowNodeModuleFiles
            ? allFilePaths
            : allFilePaths.filter(filePath => !isInNodeModules.test(filePath));
    }
}
