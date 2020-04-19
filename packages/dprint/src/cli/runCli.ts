import { Environment } from "../environment";
import { ConfigurationDiagnostic, WebAssemblyPlugin } from "@dprint/types";
import { formatFileText, resolveConfiguration } from "@dprint/core";
import { getMissingProjectTypeDiagnostic } from "../configuration";
import { parseCommandLineArgs } from "./parseCommandLineArgs";
import { getHelpText } from "./getHelpText";
import { getVersionText } from "./getVersionText";
import { CommandLineOptions } from "./CommandLineOptions";
import { KillSafeFileWriter } from "./fileSystem";
import { resolveConfigFile, resolveConfigFilePath } from "./resolveConfigFile";

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
        environment.log(getHelpText(await safeGetPlugins()));
        return;
    }
    else if (options.showVersion) {
        environment.log(getVersionText(await safeGetPlugins()));
        return;
    }
    else if (options.init) {
        await createConfigFile(environment);
        return;
    }

    const { unresolvedConfiguration, configFilePath, plugins } = await getUnresolvedConfigAndPlugins();
    try {
        await runCliWithPlugins();
    } finally {
        plugins.forEach(p => (p as WebAssemblyPlugin).dispose?.());
    }

    if (options.duration) {
        const durationInSeconds = ((new Date()).getTime() - startDate.getTime()) / 1000;
        environment.log(`Duration: ${durationInSeconds.toFixed(2)}s`);
    }

    async function runCliWithPlugins() {
        const globalConfig = resolveGlobalConfigurationInternal();
        updatePluginsWithConfiguration();
        const filePaths = await getFilePaths();

        if (options.outputResolvedConfig) {
            outputResolvedConfiguration();
            return;
        }
        else if (options.outputFilePaths) {
            if (filePaths.length > 0) {
                for (const filePath of filePaths)
                    environment.log(filePath);
            }
            else {
                environment.log("Found 0 files.");
            }
            return;
        }

        const promises: Promise<void>[] = [];
        const killSafeFileWriter = new KillSafeFileWriter(environment);
        try {
            for (const filePath of filePaths) {
                const promise = environment.readFile(filePath).then(fileText => {
                    const result = formatFileText({
                        filePath,
                        fileText,
                        plugins,
                    });
                    // skip writing the file if it hasn't changed
                    return result === false ? Promise.resolve() : killSafeFileWriter.writeFile(filePath, result);
                }).catch(err => {
                    const errorText = err.toString().replace("[dprint]: ", "");
                    environment.error(`Error formatting file: ${filePath}\n\n${errorText}`);
                });
                promises.push(promise);
            }

            await Promise.all(promises);
        } finally {
            await killSafeFileWriter.dispose();
        }

        function updatePluginsWithConfiguration() {
            for (const plugin of plugins) {
                plugin.initialize({
                    environment,
                    globalConfig,
                });

                for (const diagnostic of plugin.getConfigurationDiagnostics())
                    warnForConfigurationDiagnostic(diagnostic);
            }
        }

        function outputResolvedConfiguration() {
            environment.log(getText());

            function getText() {
                let text = `Global configuration: ${prettyPrintAsJson(globalConfig)}`;
                for (const plugin of plugins)
                    text += `\n${plugin.name}: ${prettyPrintAsJson(plugin.getConfiguration())}`;
                return text;
            }

            function prettyPrintAsJson(obj: any) {
                const numSpaces = 2;
                return JSON.stringify(obj, null, numSpaces);
            }
        }
    }

    function resolveGlobalConfigurationInternal() {
        const missingProjectTypeDiagnostic = getMissingProjectTypeDiagnostic(unresolvedConfiguration);
        const configResult = resolveConfiguration(getUnresolvedConfigStrippedOfCliSpecificConfig());

        for (const diagnostic of configResult.diagnostics)
            warnForConfigurationDiagnostic(diagnostic);

        if (missingProjectTypeDiagnostic)
            warnForConfigurationDiagnostic(missingProjectTypeDiagnostic);

        return configResult.config;

        function getUnresolvedConfigStrippedOfCliSpecificConfig() {
            const obj = { ...unresolvedConfiguration };
            delete obj.excludes;
            delete obj.includes;
            return obj;
        }
    }

    async function getFilePaths() {
        const isInNodeModules = /[\/|\\]node_modules[\/|\\]/i;
        const allFilePaths = await environment.glob(getFileGlobs());

        return options.allowNodeModuleFiles ? allFilePaths : allFilePaths.filter(filePath => !isInNodeModules.test(filePath));

        function getFileGlobs() {
            return [...getIncludes(), ...getExcludes()];

            function getIncludes() {
                if (options.filePatterns.length > 0) {
                    if (!options.outputFilePaths && unresolvedConfiguration.includes && unresolvedConfiguration.includes.length > 0)
                        environment.warn("Ignoring the configuration file's includes because file patterns were provided to the command line.");

                    return options.filePatterns;
                }

                return unresolvedConfiguration.includes || [];
            }

            function getExcludes() {
                if (!unresolvedConfiguration.excludes)
                    return [];

                // negate the pattern if it's not already negated
                return unresolvedConfiguration.excludes.map(pattern => {
                    if (pattern.startsWith("!"))
                        return pattern;
                    return "!" + pattern;
                });
            }
        }
    }

    function warnForConfigurationDiagnostic(diagnostic: ConfigurationDiagnostic) {
        environment.warn(`[${environment.basename(configFilePath)}]: ${diagnostic.message}`);
    }

    async function safeGetPlugins() {
        try {
            return (await getUnresolvedConfigAndPlugins()).plugins;
        } catch (err) {
            return [];
        }
    }

    async function getUnresolvedConfigAndPlugins() {
        const { config: unresolvedConfiguration, filePath: configFilePath } = await resolveConfigFile(options.config, environment);
        return {
            unresolvedConfiguration,
            configFilePath,
            plugins: unresolvedConfiguration.plugins || [],
        };
    }
}

async function createConfigFile(environment: Environment) {
    const filePath = resolveConfigFilePath(undefined, environment);
    if (await environment.exists(filePath)) {
        environment.warn(`Skipping initialization because a configuration file already exists at: ${filePath}`);
        return;
    }

    environment.writeFile(filePath, getDefaultConfigFileText());
    environment.log(`Created ${filePath}`);

    function getDefaultConfigFileText() {
        return `// @ts-check
const { TypeScriptPlugin } = require("dprint-plugin-typescript");
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({
        }),
        new JsoncPlugin({
            indentWidth: 2,
        }),
    ],
    includes: [
        "**/*.{ts,tsx,json,js,jsx}",
    ],
};
`;
    }
}
