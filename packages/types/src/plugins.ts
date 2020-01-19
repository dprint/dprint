import { PrintItemIterable } from "./printing";
import { BaseResolvedConfiguration, ConfigurationDiagnostic, ResolvedConfiguration } from "./configuration";
import { LoggingEnvironment } from "./environment";

/** Options for initializing a plugin. */
export interface PluginInitializeOptions {
    /** Environment to use for logging. */
    environment: LoggingEnvironment;
    /** The resolved global configuration. */
    globalConfig: ResolvedConfiguration;
}

/** Base interface a plugin must implement. */
export interface BasePlugin<ResolvedPluginConfiguration = BaseResolvedConfiguration> {
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
}

/**
 * A plugin that only lives in JavaScript land.
 */
export interface JsPlugin<ResolvedPluginConfiguration extends BaseResolvedConfiguration = BaseResolvedConfiguration>
    extends BasePlugin<ResolvedPluginConfiguration>
{
    /**
     * Parses the file to an iterable of print items.
     * @returns An iterable of print items or false if the file said to skip parsing (ex. it had an ignore comment).
     */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false;
}

/**
 * A plugin that may send the string to WebAssembly, in which it will print out the print items.
 */
export interface WebAssemblyPlugin<ResolvedPluginConfiguration extends BaseResolvedConfiguration = BaseResolvedConfiguration>
    extends BasePlugin<ResolvedPluginConfiguration>
{
    /**
     * Formats the provided file text.
     * @returns The formatted text or false if the file said to skip parsing (ex. it had an ignore comment).
     */
    formatText(filePath: string, fileText: string): string | false;
    /**
     * Disposes any unmanaged resources held by the plugin.
     */
    dispose(): void;
}

export type Plugin = WebAssemblyPlugin | JsPlugin;

/**
 * Gets if the provided plugin is a js plugin.
 * @param plugin - Plugin to check.
 */
export function isJsPlugin(plugin: Plugin): plugin is JsPlugin {
    return (plugin as any as JsPlugin).parseFile != null;
}

/**
 * Gets if the provided plugin is a WebAssembly plugin.
 * @param plugin - Plugin to check.
 */
export function isWebAssemblyPlugin(plugin: Plugin): plugin is WebAssemblyPlugin {
    return (plugin as any as WebAssemblyPlugin).formatText != null;
}
