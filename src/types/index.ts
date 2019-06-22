export type PrintItem = Behaviour | string | Group | Unknown | Condition | Info;

// iterators should only be used in groups so that they can become resetable
export interface PrintItemIterator extends Iterable<PrintItem> {
}

export enum PrintItemKind {
    Unknown,
    Group,
    Condition,
    Info
}

export interface Unknown {
    kind: PrintItemKind.Unknown,
    text: string;
}

export enum Behaviour {
    NewLine,
    SpaceOrNewLine,
    /** Expect the next character to be a newline. If it's not, force a newline */
    ExpectNewLine,
    StartIndent,
    FinishIndent,
    StartHangingIndent,
    FinishHangingIndent
}

export interface Condition {
    kind: PrintItemKind.Condition,
    condition: ResolveCondition | Condition;
    true?: PrintItemIterator;
    false?: PrintItemIterator;
}

export interface ResolveConditionContext {
    isConditionTrue(condition: Condition): boolean;
    getResolvedInfo(info: Info): WriterInfo;
    writerInfo: WriterInfo;
}

export type ResolveCondition = (context: ResolveConditionContext) => boolean;

export interface Group {
    kind: PrintItemKind.Group,
    items: PrintItemIterator;
}

export interface Info {
    kind: PrintItemKind.Info,
}

export interface WriterInfo {
    lineNumber: number;
    lineStartIndentLevel: number;
    columnNumber: number;
}
