import { PrintItem, CommentBlock, Group, GroupSeparatorKind, PrintItemKind, Separator, Unknown } from "../types";
import { assertNever, throwError } from "../utils";

interface Context {
    writer: Writer;
    state: {
        groupStartLineNumber: number;
        groupIndentationLevel: number;
        groupSeparatorKind: GroupSeparatorKind | undefined;
    },
    options: PrintOptions;
}

export interface PrintOptions {
    maxWidth: number;
    indentSize: number; // todo: support tabs
    newLineKind: "\r\n" | "\n";
}

export function print(group: Group, options: PrintOptions) {
    const context: Context = {
        writer: new Writer({ indentSize: options.indentSize, maxWidth: options.maxWidth, newLineKind: options.newLineKind }),
        state: {
            groupStartLineNumber: 0,
            groupIndentationLevel: 0,
            groupSeparatorKind: group.separatorKind
        },
        options
    };

    printGroup(group, context);

    return context.writer.toString();
}

function printGroup(group: Group, context: Context) {
    const previousState = context.state;

    context.state = {
        groupStartLineNumber: context.writer.getLineNumber(),
        groupIndentationLevel: context.writer.getIndentationLevel(),
        groupSeparatorKind: group.separatorKind
    };

    try {
        if (group.hangingIndent) {
            context.writer.hangingIndent(() => {
                printPrintItem(group.items, context);
            });
        }
        else if (group.indent) {
            context.writer.indent(() => {
                printPrintItem(group.items, context);
            });
        }
        else {
            printPrintItem(group.items, context);
        }
    }
    finally {
        context.state = previousState;
    }
}

function printPrintItem(printItem: PrintItem, context: Context) {
    if (typeof printItem === "number")
        printSeparator(printItem, context);
    else if (typeof printItem === "string")
        printString(printItem, context);
    else if (printItem instanceof Array)
        printItem.forEach(item => printPrintItem(item, context));
    else if (printItem.kind === PrintItemKind.CommentBlock)
        printCommentBlock(printItem, context);
    else if (printItem.kind === PrintItemKind.Group)
        printGroup(printItem, context);
    else if (printItem.kind === PrintItemKind.Unknown)
        printUnknown(printItem, context);
    else
        assertNever(printItem);
}

function printSeparator(separator: Separator, context: Context) {
    const { groupSeparatorKind } = context.state;
    const isInHangingIndent = context.writer.getLastLineIndentLevel() > context.state.groupIndentationLevel;

    if (separator === Separator.NewLineIfHangingSpaceOtherwise) {
        if (isInHangingIndent)
            context.writer.write(context.options.newLineKind);
        else {
            context.writer.markSpaceToConvertToNewLineIfHanging();
        }
    }
    else if (groupSeparatorKind === GroupSeparatorKind.NewLines && (separator === Separator.NewLine || separator === Separator.SpaceOrNewLine))
        context.writer.write(context.options.newLineKind);
    else if (separator === Separator.SpaceOrNewLine)
        context.writer.markSpaceOrNewLine();
}

function printCommentBlock(comment: CommentBlock, context: Context) {
}

function printUnknown(unknown: Unknown, context: Context) {
    context.writer.baseWrite(unknown.text);
}

function printString(text: string, context: Context) {
    context.writer.write(text);
}

interface SpaceMark {
    itemsIndex: number;
    indentLevel: number;
    hangingIndentLevel: number | undefined;
    lineColumn: number;
}

class Writer {
    private readonly items: string[] = [];
    private readonly singleIndentationText: string;
    private readonly spaceIndexesToConvertToNewLineOnHanging: number[] = []; // yeah, this code is bad and needs improvement

    private currentLineColumn = 0;
    private currentLineNumber = 0;
    private indentLevel = 0;
    private indentText = "";
    private hangingIndentLevel: number | undefined;
    private lastSpaceMark: SpaceMark | undefined;
    private lastLineIndentLevel = 0;

    constructor(private readonly options: { indentSize: number; maxWidth: number; newLineKind: "\r\n" | "\n" }) {
        this.singleIndentationText = " ".repeat(options.indentSize);
    }

    write(text: string) {
        const startsWithNewLine = text[0] === "\r" || text[0] === "\n";
        if (startsWithNewLine) {
            if (text !== "\n" && text !== "\r\n")
                throwError(`Text cannot be written with newlines: ${text}`);

            if (this.hangingIndentLevel != null) {
                this.setIndentationLevel(this.hangingIndentLevel);
                this.hangingIndentLevel = undefined;
            }
        }

        if (this.currentLineColumn === 0 && !startsWithNewLine && this.indentLevel > 0)
            this.baseWrite(this.indentText);

        this.baseWrite(text);
    }

    private splitIfOver(lineColumn: number) {
        const lastSpaceMark = this.lastSpaceMark;
        if (lastSpaceMark == null || lineColumn < this.options.maxWidth)
            return false;

        // save the state
        const originalHangingIndentLevel = this.hangingIndentLevel;
        const originalIndentLevel = this.indentLevel;

        // skip writing
        const spaceIndexesToConvertToNewLineOnHanging = this.spaceIndexesToConvertToNewLineOnHanging.map(index => index - lastSpaceMark.itemsIndex);
        const reWriteItems = this.items.splice(lastSpaceMark.itemsIndex, this.items.length - lastSpaceMark.itemsIndex);
        this.lastSpaceMark = undefined;
        this.currentLineColumn = lastSpaceMark.lineColumn;

        this.hangingIndentLevel = lastSpaceMark.hangingIndentLevel;
        this.setIndentationLevel(lastSpaceMark.indentLevel);

        // rewrite everything into the writer on the next line
        this.write(this.options.newLineKind);
        for (let i = 1 /* skip space */; i < reWriteItems.length; i++) {
            if (lastSpaceMark.hangingIndentLevel != null && spaceIndexesToConvertToNewLineOnHanging.includes(i))
                this.write(this.options.newLineKind);
            else
                this.write(reWriteItems[i]);
        }

        // restore the state
        if (originalHangingIndentLevel != null)
            this.setIndentationLevel(originalHangingIndentLevel);
        else
            this.setIndentationLevel(originalIndentLevel);

        if (originalHangingIndentLevel == null || originalHangingIndentLevel <= this.indentLevel)
            this.hangingIndentLevel = undefined;

        return true;
    }

    baseWrite(text: string) {
        for (let i = 0; i < text.length; i++) {
            if (this.splitIfOver(this.currentLineColumn)) {
                this.write(text);
                return;
            }

            if (text[i] === "\n") {
                this.lastSpaceMark = undefined;
                this.spaceIndexesToConvertToNewLineOnHanging.length = 0;
                this.currentLineColumn = 0;
                this.currentLineNumber++;
                this.lastLineIndentLevel = this.indentLevel;
            }
            else
                this.currentLineColumn++;
        }

        this.items.push(text);
    }

    indent(duration: () => void) {
        const originalHangingIndentLevel = this.hangingIndentLevel;
        const originalLevel = this.indentLevel;
        this.setIndentationLevel(this.indentLevel + 1);
        try {
            duration();
        } finally {
            this.hangingIndentLevel = originalHangingIndentLevel;
            this.setIndentationLevel(originalLevel);
        }
    }

    hangingIndent(duration: () => void) {
        const originalHangingIndentLevel = this.hangingIndentLevel;
        const originalLevel = this.indentLevel;
        this.hangingIndentLevel = this.indentLevel + 1;
        try {
            duration();
        } finally {
            this.hangingIndentLevel = originalHangingIndentLevel;
            this.setIndentationLevel(originalLevel);
        }
    }

    markSpaceOrNewLine() {
        this.lastSpaceMark = {
            itemsIndex: this.items.length,
            indentLevel: this.indentLevel,
            hangingIndentLevel: this.hangingIndentLevel,
            lineColumn: this.getLineColumn()
        };
        this.write(" ");
    }

    markSpaceToConvertToNewLineIfHanging() {
        const lastSpaceMark = this.lastSpaceMark;
        if (this.splitIfOver(this.currentLineColumn + 1) && lastSpaceMark != null && lastSpaceMark.hangingIndentLevel != null) {
            this.write(this.options.newLineKind);
        }
        else {
            this.spaceIndexesToConvertToNewLineOnHanging.push(this.items.length);
            this.write(" ");
        }
    }

    getLastLineIndentLevel() {
        return this.lastLineIndentLevel;
    }

    getIndentationLevel() {
        return this.indentLevel;
    }

    /** Gets the zero-indexed line column. */
    getLineColumn() {
        if (this.currentLineColumn === 0)
            return this.indentText.length;
        return this.currentLineColumn;
    }

    /** Gets the zero-index line number. */
    getLineNumber() {
        return this.currentLineNumber;
    }

    toString() {
        return this.items.join("");
    }

    private setIndentationLevel(level: number) {
        if (this.indentLevel === level)
            return;

        this.indentLevel = level;
        this.indentText = this.singleIndentationText.repeat(level);
    }
}
