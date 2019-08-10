/**
 * The different items that the printer could encounter.
 */
export type PrintItem = Signal | string | RawString | Condition | Info;

/**
 * An iterable of print items.
 */
export interface PrintItemIterable extends Iterable<PrintItem> {
}

/**
 * The kind of print item.
 */
export enum PrintItemKind {
    RawString,
    Condition,
    Info
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
export enum Signal {
    /**
     * Signal that the current location could be a newline when
     * exceeding the print width.
     */
    NewLine,
    /**
     * Signal that the current location should be a space, but
     * could be a newline if exceeding the print width.
     */
    SpaceOrNewLine,
    /**
     * Expect the next character to be a newline. If it's not, force a newline.
     */
    ExpectNewLine,
    /**
     * Signal the start of a section that should be indented.
     */
    StartIndent,
    /**
     * Signal the end of a section that should be indented.
     */
    FinishIndent,
    /**
     * Signal the start of a group of print items that have a lower precedence
     * for being broken up with a newline for exceeding the line width.
     */
    StartNewlineGroup,
    /**
     * Signal the end of a newline group.
     */
    FinishNewLineGroup,
    /**
     * Signal that a single indent should occur based on the printer settings.
     */
    SingleIndent,
    /**
     * Signal to the writer that it should stop using indentation.
     */
    StartIgnoringIndent,
    /**
     * Signal to the writer that it should start using indentation again.
     */
    FinishIgnoringIndent
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

export type ResolveCondition = (context: ResolveConditionContext) => boolean | undefined;

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
