// dprint-ignore-file

export declare const version = "0.4.4";

export declare function makeIterableRepeatable<T>(iterable: Iterable<T>): Iterable<T>;

export declare function getFileExtension(filePath: string): string;

/**
 * Gets the last newline character from the provided text or returns the
 * system's newline character if no newline exists.
 * @param text - Text to inspect.
 */
export declare function resolveNewLineKindFromText(text: string): "\r\n" | "\n";

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
    newlineKind?: "auto" | "crlf" | "lf" | "system";
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
    readonly newlineKind: "auto" | "\r\n" | "\n";
}

/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: string;
    /** The diagnostic's message that should be displayed to the user. */
    message: string;
}

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
 * The different items that the printer could encounter.
 */
export declare type PrintItem = Signal | string | RawString | Condition | Info;

/**
 * An iterable of print items.
 */
export interface PrintItemIterable extends Iterable<PrintItem> {
}

/**
 * The kind of print item.
 */
export declare enum PrintItemKind {
    RawString = 0,
    Condition = 1,
    Info = 2
}

/**
 * Represents a string that should be formatted as-is.
 */
export interface RawString {
    kind: PrintItemKind.RawString;
    text: string;
}

/**
 * Signals for the printer.
 */
export declare enum Signal {
    /**
     * Signal that the current location could be a newline when
     * exceeding the print width.
     */
    NewLine = 0,
    /**
     * Signal that the current location should be a space, but
     * could be a newline if exceeding the print width.
     */
    SpaceOrNewLine = 1,
    /**
     * Expect the next character to be a newline. If it's not, force a newline.
     */
    ExpectNewLine = 2,
    /**
     * Signal the start of a section that should be indented.
     */
    StartIndent = 3,
    /**
     * Signal the end of a section that should be indented.
     */
    FinishIndent = 4,
    /**
     * Signal the start of a group of print items that have a lower precedence
     * for being broken up with a newline for exceeding the line width.
     */
    StartNewlineGroup = 5,
    /**
     * Signal the end of a newline group.
     */
    FinishNewLineGroup = 6,
    /**
     * Signal that a single indent should occur based on the printer settings.
     */
    SingleIndent = 7,
    /**
     * Signal to the writer that it should stop using indentation.
     */
    StartIgnoringIndent = 8,
    /**
     * Signal to the writer that it should start using indentation again.
     */
    FinishIgnoringIndent = 9
}

/**
 * Conditionally print items based on a condition.
 *
 * These conditions are extremely flexible and could be resolved based on
 * information found later on in the file.
 */
export interface Condition {
    kind: PrintItemKind.Condition;
    /** Name for debugging purposes. */
    name: string;
    /** The condition to resolve or another condition to base this condition on. */
    condition: ResolveCondition | Condition;
    /** The items to print when the condition is true. */
    true?: PrintItemIterable;
    /** The items to print when the condition is false or undefined (not yet resolved). */
    false?: PrintItemIterable;
}

export declare type ResolveCondition = (context: ResolveConditionContext) => boolean | undefined;

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
 * Can be used to get information at a certain location being printed. These can be resolved
 * by providing the info object to a condition context's getResolvedInfo method.
 */
export interface Info {
    kind: PrintItemKind.Info;
    /** Name for debugging purposes. */
    name: string;
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

export interface BaseContext {
    fileText: string;
    newlineKind: "\r\n" | "\n";
}

export declare namespace conditionResolvers {
    function isStartOfNewLine(conditionContext: ResolveConditionContext): boolean;
    function isHanging(conditionContext: ResolveConditionContext, startInfo: Info, endInfo?: Info): boolean | undefined;
    function isMultipleLines(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info | WriterInfo, defaultValue?: boolean): boolean | undefined;
    function areInfoEqual(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info, defaultValue: boolean): boolean;
}

/** A collection of reusable conditions. */
export declare namespace conditions {
    interface NewlineIfHangingSpaceOtherwiseOptions {
        context: BaseContext;
        startInfo: Info;
        endInfo?: Info;
        spaceChar?: " " | Signal.SpaceOrNewLine;
    }
    function newlineIfHangingSpaceOtherwise(options: NewlineIfHangingSpaceOtherwiseOptions): Condition;
    interface NewlineIfMultipleLinesSpaceOrNewlineOtherwiseOptions {
        context: BaseContext;
        startInfo: Info;
        endInfo?: Info;
    }
    function newlineIfMultipleLinesSpaceOrNewlineOtherwise(options: NewlineIfMultipleLinesSpaceOrNewlineOtherwiseOptions): Condition;
    function singleIndentIfStartOfLine(): Condition;
    function indentIfStartOfLine(item: PrintItemIterable): PrintItemIterable;
    function withIndentIfStartOfLineIndented(item: PrintItemIterable): PrintItemIterable;
    /**
     * This condition can be used to force the printer to jump back to the point
     * this condition exists at once the provided info is resolved.
     * @param info - Info to force reevaluation once resolved.
     */
    function forceReevaluationOnceResolved(info: Info): Condition;
}

export declare namespace parserHelpers {
    function withIndent(item: PrintItemIterable): PrintItemIterable;
    function newlineGroup(item: PrintItemIterable): PrintItemIterable;
    function prependToIterableIfHasItems<T>(iterable: Iterable<T>, ...items: T[]): IterableIterator<T>;
    function toPrintItemIterable(printItem: PrintItem): PrintItemIterable;
    function surroundWithNewLines(item: PrintItemIterable, context: BaseContext): PrintItemIterable;
    /**
     * Reusable function for parsing a js-like single line comment (ex. // comment)
     * @param rawCommentValue - The comment value without the leading two slashes.
     */
    function parseJsLikeCommentLine(rawCommentValue: string): string;
    function createInfo(name: string): Info;
}

/**
 * An implementation of an environment that outputs to the console.
 */
export declare class CliLoggingEnvironment implements LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
}

/** Represents an execution environment. */
export interface LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
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

export declare function formatFileText(options: FormatFileTextOptions): string;

export interface FormatFileTextOptions {
    filePath: string;
    fileText: string;
    plugins: Plugin[];
}
