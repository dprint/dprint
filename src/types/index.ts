// trick to do circular reference https://github.com/Microsoft/TypeScript/issues/3496#issuecomment-128553540
export type PrintItem = Separator | string | Group | Unknown | PrintItemIterator;

export interface PrintItemIterator extends IterableIterator<PrintItem> {
}

export enum PrintItemKind {
    Unknown,
    Group
}

export interface Unknown {
    kind: PrintItemKind.Unknown,
    text: string;
}

export enum GroupBehaviour {
    Indent,
    Hanging,
    MultipleLines,
    MultipleLinesWithIndent
}

export enum Separator {
    NewLine,
    SpaceOrNewLine,

    // Special cases
    NewLineIfHangingSpaceOtherwise,
    /** Expect the next character to be a newline. If it's not, force a newline */
    ExpectNewLine
}

export interface Group {
    kind: PrintItemKind.Group,
    hangingIndent?: boolean;
    indent?: boolean;
    separatorKind?: GroupSeparatorKind;
    items: PrintItemIterator;
}

export enum GroupSeparatorKind {
    /** Use spaces if doing multiple lines. */
    Spaces,
    /** Use newlines if doing multiple lines. */
    NewLines
}
