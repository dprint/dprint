// dprint-ignore-file

/**
 * Dprint's configuration.
 */
export interface Configuration {
    /**
     * Specify the type of project this is. You may specify any of the allowed values here according to your conscience.
     * @value "openSource" - Dprint is formatting an open source project.
     * @value "commercialSponsored" - Dprint is formatting a closed source commercial project and your company sponsored dprint.
     * @value "commercialDidNotSponsor" - Dprint is formatting a closed source commercial project and you want to forever enshrine your name in source control for having specified this.
     */
    projectType: "openSource" | "commercialSponsored" | "commercialDidNotSponsor";
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
    newLineKind?: "auto" | "crlf" | "lf" | "system";
    /**
     * Collection of plugins to use.
     */
    plugins: Plugin[];
}

export interface ResolvedConfiguration extends BaseResolvedConfiguration {
}

export interface BaseResolvedConfiguration {
    readonly lineWidth: number;
    readonly indentWidth: number;
    readonly useTabs: boolean;
    readonly newLineKind: "auto" | "\r\n" | "\n";
}

/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: string;
    /** The diagnostic's message that should be displayed to the user. */
    message: string;
}

/** Represents an execution environment. */
export interface LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
}

/**
 * The different items the printer could encounter.
 */
export declare type PrintItem = Signal | string | Condition | Info;

/**
 * An iterable of print items.
 */
export interface PrintItemIterable extends Iterable<PrintItem> {
}

/**
 * The kind of print item.
 */
export declare enum PrintItemKind {
    Condition = 0,
    Info = 1
}

/**
 * Signals for the printer.
 */
export declare enum Signal {
    /**
     * Signal that a new line should occur based on the printer settings.
     */
    NewLine = 0,
    /**
     * Signal that a tab should occur.
     */
    Tab = 1,
    /**
     * Signal that the current location could be a newline when
     * exceeding the line width.
     */
    PossibleNewLine = 2,
    /**
     * Signal that the current location should be a space, but
     * could be a newline if exceeding the line width.
     */
    SpaceOrNewLine = 3,
    /**
     * Expect the next character to be a newline. If it's not, force a newline.
     */
    ExpectNewLine = 4,
    /**
     * Signal the start of a section that should be indented.
     */
    StartIndent = 5,
    /**
     * Signal the end of a section that should be indented.
     */
    FinishIndent = 6,
    /**
     * Signal the start of a group of print items that have a lower precedence
     * for being broken up with a newline for exceeding the line width.
     */
    StartNewLineGroup = 7,
    /**
     * Signal the end of a newline group.
     */
    FinishNewLineGroup = 8,
    /**
     * Signal that a single indent should occur based on the printer settings.
     */
    SingleIndent = 9,
    /**
     * Signal to the printer that it should stop using indentation.
     */
    StartIgnoringIndent = 10,
    /**
     * Signal to the printer that it should start using indentation again.
     */
    FinishIgnoringIndent = 11
}

/**
 * Can be used to get information at a certain location being printed. These can be resolved
 * by providing the info object to a condition context's getResolvedInfo method.
 */
export interface Info {
    kind: PrintItemKind.Info;
    /** Name for debugging purposes. */
    name: string;
}

/**
 * Conditionally print items based on a condition.
 *
 * These conditions are extremely flexible and can even be resolved based on
 * information found later on in the file.
 */
export interface Condition {
    kind: PrintItemKind.Condition;
    /** Name for debugging purposes. */
    name: string;
    /** The condition to resolve or another condition to base this condition on. */
    condition: ConditionResolver | Condition;
    /** The items to print when the condition is true. */
    true?: PrintItemIterable;
    /** The items to print when the condition is false or undefined (not yet resolved). */
    false?: PrintItemIterable;
}

/**
 * Function used to resolve a condition.
 */
export declare type ConditionResolver = (context: ResolveConditionContext) => boolean | undefined;

/**
 * Context used when resolving a condition.
 */
export interface ResolveConditionContext {
    /**
     * Gets if a condition was true, false, or returns undefined when not yet resolved.
     */
    getResolvedCondition(condition: Condition): boolean | undefined;
    /**
     * Gets if a condition was true, false, or returns the provided default value when
     * not yet resolved.
     */
    getResolvedCondition(condition: Condition, defaultValue: boolean): boolean;
    /**
     * Gets the writer info at a specified info or returns undefined when not yet resolved.
     */
    getResolvedInfo(info: Info): WriterInfo | undefined;
    /**
     * Gets the writer info at the condition's location.
     */
    writerInfo: WriterInfo;
}

/**
 * Information about a certain location being printed.
 */
export interface WriterInfo {
    lineNumber: number;
    columnNumber: number;
    indentLevel: number;
    lineStartIndentLevel: number;
    lineStartColumnNumber: number;
}

/** Options for initializing a plugin. */
export interface PluginInitializeOptions {
    /** Environment to use for logging. */
    environment: LoggingEnvironment;
    /** The resolved global configuration. */
    globalConfig: ResolvedConfiguration;
}

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
     * Initializes the plugin for use.
     * @remarks Plugins should be resilient to this never being called.
     */
    initialize(options: PluginInitializeOptions): void;
    /**
     * Gets whether the plugin should parse the file.
     */
    shouldParseFile(filePath: string, fileText: string): boolean;
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
