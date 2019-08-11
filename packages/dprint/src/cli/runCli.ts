import { Environment } from "../environment";
import { formatFileText, ConfigurationDiagnostic, resolveConfiguration } from "@dprint/core";
import { parseCommandLineArgs } from "./parseCommandLineArgs";
import { getHelpText } from "./getHelpText";
import { getPackageVersion } from "./getPackageVersion";
import { CommandLineOptions } from "./CommandLineOptions";
import { resolveConfigFile } from "./resolveConfigFile";
import { getMissingProjectTypeDiagnostic } from "../configuration";

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
    const startDate = new Date();

    if (options.showHelp) {
        // todo: log plugins
        environment.log(getHelpText());
        return;
    }
    else if (options.showVersion) {
        // todo: log plugin versions
        environment.log(getPackageVersion());
        return;
    }

    const { config: unresolvedConfiguration, filePath: configFilePath } = await resolveConfigFile(options.config, environment);
    const plugins = unresolvedConfiguration.plugins || [];
    const globalConfig = resolveGlobalConfigurationInternal();
    updatePluginsWithConfiguration();
    const filePaths = await getFilePaths();

    if (options.outputFilePaths) {
        for (const filePath of filePaths)
            environment.log(filePath);
        return;
    }
    else if (options.outputResolvedConfig) {
        outputResolvedConfiguration();
        return;
    }

    const promises: Promise<void>[] = [];

    for (const filePath of filePaths) {
        const promise = environment.readFile(filePath).then(fileText => {
            const result = formatFileText({
                filePath,
                fileText,
                plugins
            });
            // skip writing the file if it hasn't changed
            return result === fileText ? Promise.resolve() : environment.writeFile(filePath, result);
        }).catch(err => {
            const errorText = err.toString().replace("[dprint]: ", "");
            environment.error(`Error formatting file: ${filePath}\n\n${errorText}`);
        });
        promises.push(promise);
    }

    return Promise.all(promises).then(() => {
        if (options.duration) {
            const durationInSeconds = ((new Date()).getTime() - startDate.getTime()) / 1000;
            environment.log(`Duration: ${durationInSeconds.toFixed(2)}s`);
        }
    });

    function resolveGlobalConfigurationInternal() {
        const missingProjectTypeDiagnostic = getMissingProjectTypeDiagnostic(unresolvedConfiguration);
        const configResult = resolveConfiguration(unresolvedConfiguration);

        for (const diagnostic of configResult.diagnostics)
            warnForConfigurationDiagnostic(diagnostic);

        if (missingProjectTypeDiagnostic)
            warnForConfigurationDiagnostic(missingProjectTypeDiagnostic);

        return configResult.config;
    }

    function updatePluginsWithConfiguration() {
        for (const plugin of plugins) {
            plugin.setGlobalConfiguration(globalConfig);

            for (const diagnostic of plugin.getConfigurationDiagnostics())
                warnForConfigurationDiagnostic(diagnostic);
        }
    }

    async function getFilePaths() {
        const isInNodeModules = /[\/|\\]node_modules[\/|\\]/i;
        const allFilePaths = await environment.glob(options.filePatterns);

        return options.allowNodeModuleFiles
            ? allFilePaths
            : allFilePaths.filter(filePath => !isInNodeModules.test(filePath));
    }

    function warnForConfigurationDiagnostic(diagnostic: ConfigurationDiagnostic) {
        environment.warn(`[${environment.basename(configFilePath)}]: ${diagnostic.message}`);
    }

    function outputResolvedConfiguration() {
        environment.log(getText());

        function getText() {
            // todo: format json here
            let text = `Global configuration: ${JSON.stringify(globalConfig)}`;
            for (const plugin of plugins)
                text += `\n${plugin.name}: ${JSON.stringify(plugin.getConfiguration())}`;
            return text;
        }
    }
}
