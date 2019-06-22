export type PrintItem = Separator | string | Group | Unknown | Condition;

// iterators should only be used in groups so that they can become resetable
export interface PrintItemIterator extends Iterable<PrintItem> {
}

export enum PrintItemKind {
    Unknown,
    Group,
    Condition
}

export interface Unknown {
    kind: PrintItemKind.Unknown,
    text: string;
}

export enum Separator {
    NewLine,
    SpaceOrNewLine,

    // Special cases
    NewLineIfHangingSpaceOtherwise,//todo: remove
    /** Expect the next character to be a newline. If it's not, force a newline */
    ExpectNewLine
}

export interface Condition {
    kind: PrintItemKind.Condition,
    condition: ConditionKind | Condition;
    true?: PrintItem | PrintItemIterator;
    false?: PrintItem | PrintItemIterator;
}

export enum ConditionKind {
    Hanging
}

export interface Group {
    kind: PrintItemKind.Group,
    hangingIndent?: boolean;
    indent?: boolean;
    items: PrintItemIterator;
}

export enum GroupSeparatorKind {
    /** Use spaces if doing multiple lines. */
    Spaces,
    /** Use newlines if doing multiple lines. */
    NewLines
}
