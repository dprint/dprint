// dprint-ignore-file
import { Plugin, Configuration, ConfigurationDiagnostic, ResolvedConfiguration, BaseResolvedConfiguration, LoggingEnvironment } from "@dprint/types";

export declare const version = "0.9.0";

export declare function getFileExtension(filePath: string): string;

/** The result of resolving configuration. */
export interface ResolveConfigurationResult<ResolvedConfiguration extends BaseResolvedConfiguration> {
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
    /** The resolved configuration. */
    config: ResolvedConfiguration;
}

/**
 * Changes the provided configuration to have all its properties resolved to a value.
 * @param config - Configuration to resolve.
 * @param pluginPropertyNames - Collection of plugin property names to ignore for excess property diagnostics.
 */
export declare function resolveConfiguration(config: Partial<Configuration>): ResolveConfigurationResult<ResolvedConfiguration>;

/**
 * An implementation of an environment that outputs to the console.
 */
export declare class CliLoggingEnvironment implements LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
}

/**
 * Formats the provided file's text.
 * @param options - Options to use.
 * @returns The file text when it's changed; false otherwise.
 */
export declare function formatFileText(options: FormatFileTextOptions): string | false;

/** Options for formatting. */
export interface FormatFileTextOptions {
    /** File path of the file to format. This will help select the plugin to use. */
    filePath: string;
    /** File text of the file to format. */
    fileText: string;
    /**
     * Plugins to use.
     * @remarks This function does not assume ownership of the plugins and so if there are
     * any plugins that require disposal then you should dispose of them after you no longer
     * need them.
     */
    plugins: Plugin[];
}
