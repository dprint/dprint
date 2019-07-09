export type PrintItem = Signal | string | Unknown | Condition | Info;

// iterators should only be used in groups so that they can become resetable
export interface PrintItemIterator extends Iterable<PrintItem> {
}

export enum PrintItemKind {
    Unknown,
    Condition,
    Info
}

export interface Unknown {
    kind: PrintItemKind.Unknown,
    text: string;
}

export enum Signal {
    NewLine,
    SpaceOrNewLine,
    /** Expect the next character to be a newline. If it's not, force a newline */
    ExpectNewLine,
    StartIndent,
    FinishIndent,
    StartHangingIndent,
    FinishHangingIndent,
    StartNewlineGroup,
    FinishNewLineGroup
}

export interface Condition {
    kind: PrintItemKind.Condition,
    /** Name for debugging purposes. */
    name: string;
    condition: ResolveCondition | Condition;
    true?: PrintItemIterator;
    false?: PrintItemIterator;
}

export interface ResolveConditionContext {
    getResolvedCondition(condition: Condition): boolean | undefined; // undefined when not yet resolved
    getResolvedCondition(condition: Condition, defaultValue: boolean): boolean;
    getResolvedInfo(info: Info): WriterInfo | undefined; // undefined when not yet resolved
    writerInfo: WriterInfo;
}

export type ResolveCondition = (context: ResolveConditionContext) => boolean | undefined;

export interface Info {
    kind: PrintItemKind.Info;
    /** Name for debugging purposes. */
    name: string;
}

export interface WriterInfo {
    lineNumber: number;
    lineStartIndentLevel: number;
    columnNumber: number;
}
