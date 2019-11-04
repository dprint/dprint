// dprint-ignore-file
import { Condition, Signal, Info, PrintItem, PrintItemIterable, WriterInfo, Plugin, Configuration, ConfigurationDiagnostic, ResolvedConfiguration, ResolveConditionContext, BaseResolvedConfiguration, LoggingEnvironment } from "@dprint/types";

export declare const version = "0.4.4";

export declare function makeIterableRepeatable<T>(iterable: Iterable<T>): Iterable<T>;

export declare function getFileExtension(filePath: string): string;

/**
 * Gets the last newline character from the provided text or returns the
 * system's newline character if no newline exists.
 * @param text - Text to inspect.
 */
export declare function resolveNewLineKindFromText(text: string): "\r\n" | "\n";

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

export declare namespace conditionResolvers {
    function isStartOfNewLine(conditionContext: ResolveConditionContext): boolean;
    function isHanging(conditionContext: ResolveConditionContext, startInfo: Info, endInfo?: Info): boolean | undefined;
    function isMultipleLines(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info | WriterInfo, defaultValue?: boolean): boolean | undefined;
    function areInfoEqual(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info, defaultValue: boolean): boolean;
}

/** A collection of reusable conditions. */
export declare namespace conditions {
    interface NewlineIfHangingSpaceOtherwiseOptions {
        startInfo: Info;
        endInfo?: Info;
        spaceChar?: " " | Signal.SpaceOrNewLine;
    }
    function newlineIfHangingSpaceOtherwise(options: NewlineIfHangingSpaceOtherwiseOptions): Condition;
    interface NewlineIfMultipleLinesSpaceOrNewlineOtherwiseOptions {
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
    function surroundWithNewLines(item: PrintItemIterable): PrintItemIterable;
    /**
     * Reusable function for parsing a js-like single line comment (ex. // comment)
     * @param rawCommentValue - The comment value without the leading two slashes.
     */
    function parseJsLikeCommentLine(rawCommentValue: string): string;
    function createInfo(name: string): Info;
    /**
     * Takes a string that could contain tabs or newlines and breaks it up into
     * the correct newlines and tabs.
     * @param text - Text to break up.
     */
    function parseRawString(text: string): PrintItemIterable;
}

/**
 * An implementation of an environment that outputs to the console.
 */
export declare class CliLoggingEnvironment implements LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
}

export declare function formatFileText(options: FormatFileTextOptions): string;

export interface FormatFileTextOptions {
    filePath: string;
    fileText: string;
    plugins: Plugin[];
}
