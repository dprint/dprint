import { BaseResolvedConfiguration, ConfigurationDiagnostic, ResolvedConfiguration } from "./configuration";
import { LoggingEnvironment } from "./environment";

/** Options for initializing a plugin. */
export interface PluginInitializeOptions {
    /** Environment to use for logging. */
    environment: LoggingEnvironment;
    /** The resolved global configuration. */
    globalConfig: ResolvedConfiguration;
}

/**
 * Plugin for dprint.
 */
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
     * Initializes the plugin for use.
     * @remarks Plugins should be resilient to this never being called.
     */
    initialize(options: PluginInitializeOptions): void;
    /**
     * Gets whether the plugin should format the file.
     */
    shouldFormatFile(filePath: string, fileText: string): boolean;
    /**
     * Gets the resolved configuration for the plugin.
     */
    getConfiguration(): ResolvedPluginConfiguration;
    /**
     * Gets the configuration diagnostics.
     */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[];
    /**
     * Formats the provided file text.
     * @returns The formatted text or false if the file said to skip parsing (ex. it had an ignore comment).
     */
    formatText(filePath: string, fileText: string): string | false;
    /**
     * Disposes any unmanaged resources held by the plugin.
     */
    dispose?(): void;
}
