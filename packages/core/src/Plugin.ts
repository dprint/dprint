import { PrintItemIterable } from "./types";
import { BaseResolvedConfiguration, ConfigurationDiagnostic, ResolvedConfiguration } from "./configuration";

/** Base interface a plugin must implement. */
export interface Plugin<ResolvedPluginConfiguration extends BaseResolvedConfiguration = BaseResolvedConfiguration> {
    /**
     * The package version of the plugin.
     */
    version: string;
    /**
     * Name of this plugin.
     */
    name: string;
    /**
     * Gets whether the plugin should parse the file.
     */
    shouldParseFile(filePath: string, fileText: string): boolean;
    /**
     * Sets the global configuration.
     */
    setGlobalConfiguration(globalConfig: ResolvedConfiguration): void;
    /**
     * Gets the resolved configuration for the plugin.
     */
    getConfiguration(): ResolvedPluginConfiguration;
    /**
     * Gets the configuration diagnostics.
     */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[];
    /**
     * Parses the file to an iterable of print items.
     * @returns An iterable of print items or false if the file said to skip parsing (ex. it had an ignore comment).
     */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false;
}
