import { PrintItemIterable, Plugin, PluginInitializeOptions, BaseResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/core";

export interface JsoncConfiguration {
    /**
     * The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
     * @default 120
     */
    lineWidth?: number;
    /**
     * The number of spaces for an indent. This option is ignored when using tabs.
     * @default 4
     */
    indentWidth?: number;
    /**
     * Whether to use tabs (true) or spaces (false).
     * @default false
     */
    useTabs?: boolean;
    /**
     * The kind of newline to use.
     * @default "auto"
     * @value "auto" - For each file, uses the newline kind found at the end of the last line.
     * @value "crlf" - Uses carriage return, line feed.
     * @value "lf" - Uses line feed.
     * @value "system" - Uses the system standard (ex. crlf on Windows).
     */
    newlineKind?: "auto" | "crlf" | "lf" | "system";
}

/**
 * Resolved configuration from user specified configuration.
 */
export interface ResolvedJsoncConfiguration extends BaseResolvedConfiguration {
}

export declare class JsoncPlugin implements Plugin<ResolvedJsoncConfiguration> {
    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config?: JsoncConfiguration);
    /** @inheritdoc */
    version: string;
    /** @inheritdoc */
    name: string;
    /** @inheritdoc */
    initialize(options: PluginInitializeOptions): void;
    /** @inheritdoc */
    shouldParseFile(filePath: string): boolean;
    /** @inheritdoc */
    getConfiguration(): ResolvedJsoncConfiguration;
    /** @inheritdoc */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[];
    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false;
}
