import { PrintItemIterable } from "../types";
import { ResolveConfigurationResult, ResolvedGlobalConfiguration } from "../configuration";

/** Base interface a plugin must implement. */
export interface Plugin<UnresolvedConfiguration = unknown, ResolvedConfiguration extends ResolvedGlobalConfiguration = ResolvedGlobalConfiguration> {
    /**
     * The package version of the plugin.
     */
    version: string;
    /**
     * Name of this plugin.
     */
    name: string;
    /**
     * The property name for configuration.
     */
    configurationPropertyName: string;
    /**
     * Gets whether the plugin should parse the file.
     */
    shouldParseFile(filePath: string, fileText: string): boolean;
    /**
     * Sets the configuration the plugin should use.
     */
    setConfiguration(globalConfig: ResolvedGlobalConfiguration, pluginConfig: UnresolvedConfiguration): ResolveConfigurationResult<ResolvedConfiguration>;
    /**
     * Gets the resolved configuration for the plugin.
     */
    getConfiguration(): ResolvedConfiguration;
    /**
     * Parses the file to an iterable of print items.
     * @returns An iterable of print items or false if the file said to skip parsing (ex. it had an ignore comment).
     */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false;
}

