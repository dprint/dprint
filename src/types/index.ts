// trick to do circular reference https://github.com/Microsoft/TypeScript/issues/3496#issuecomment-128553540
export type PrintItem = Separator | string | Group | Unknown | CommentBlock | PrintItemArray;

export interface PrintItemArray extends Array<PrintItem> {
}

export enum PrintItemKind {
    Unknown,
    Group,
    CommentBlock,
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

    /* Special cases */
    NewLineIfHangingSpaceOtherwise
}

export interface Group {
    kind: PrintItemKind.Group,
    hangingIndent?: boolean;
    indent?: boolean;
    separatorKind?: GroupSeparatorKind;
    items: PrintItem[];
}

export enum GroupSeparatorKind {
    /** Use spaces if doing multiple lines. */
    Spaces,
    /** Use newlines if doing multiple lines. */
    NewLines
}

export type Comment = CommentBlock;

export interface CommentBlock {
    kind: PrintItemKind.CommentBlock,
    isJsDoc: boolean;
    inline: boolean;
    value: string;
}
