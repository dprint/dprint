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
    const isInHangingIndent = context.writer.getIndentationLevel() > context.state.groupIndentationLevel;

    if (separator === Separator.NewLineIfHangingSpaceOtherwise) {
        if (isInHangingIndent)
            context.writer.write(context.options.newLineKind);
        else {
            context.writer.write(" ");
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
    hangingIndentLevel: number | undefined;
}

class Writer {
    private readonly items: string[] = [];
    private readonly singleIndentationText: string;
    private readonly spaceIndexesToConvertToNewLineOnHanging: number[] = []; // yeah, this code is bad and needs improvement

    private indentationLevel = 0;
    private currentLineColumn = 0;
    private currentLineNumber = 0;
    private indentationText = "";
    private hangingIndentLevel: number | undefined;
    private lastSpaceMark: SpaceMark | undefined;

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

        if (this.currentLineColumn === 0 && !startsWithNewLine)
            this.baseWrite(this.indentationText);

        this.baseWrite(text);
    }

    baseWrite(text: string) {
        const originalColumn = this.currentLineColumn;
        for (let i = 0; i < text.length; i++) {
            if (text[i] === "\n") {
                const lastSpaceMark = this.lastSpaceMark;
                if (lastSpaceMark != null && this.currentLineColumn > this.options.maxWidth) {
                    // skip writing
                    const spaceIndexesToConvertToNewLineOnHanging = this.spaceIndexesToConvertToNewLineOnHanging.map(index => index - lastSpaceMark.itemsIndex);
                    const reWriteItems = this.items.splice(lastSpaceMark.itemsIndex, this.items.length - lastSpaceMark.itemsIndex);
                    this.lastSpaceMark = undefined;
                    this.currentLineColumn = originalColumn;

                    // write the correct hanging indent level
                    const previousIndentationLevel = this.indentationLevel;
                    if (lastSpaceMark.hangingIndentLevel != null)
                        this.setIndentationLevel(lastSpaceMark.hangingIndentLevel);

                    // rewrite everything into the writer on the next line
                    this.write(this.options.newLineKind);
                    for (let i = 1 /* skip space */; i < reWriteItems.length; i++) {
                        if (lastSpaceMark.hangingIndentLevel != null && spaceIndexesToConvertToNewLineOnHanging.includes(i))
                            this.write(this.options.newLineKind);
                        else
                            this.write(reWriteItems[i]);
                    }

                    this.setIndentationLevel(previousIndentationLevel);
                    this.write(text);
                    return;
                }

                this.lastSpaceMark = undefined;
                this.spaceIndexesToConvertToNewLineOnHanging.length = 0;
                this.currentLineColumn = 0;
                this.currentLineNumber++;
            }
            else
                this.currentLineColumn++;
        }

        this.items.push(text);
    }

    indent(duration: () => void) {
        const originalHangingIndentLevel = this.hangingIndentLevel;
        const originalLevel = this.indentationLevel;
        this.setIndentationLevel(this.indentationLevel + 1);
        try {
            duration();
        } finally {
            this.hangingIndentLevel = originalHangingIndentLevel;
            this.setIndentationLevel(originalLevel);
        }
    }

    hangingIndent(duration: () => void) {
        const originalHangingIndentLevel = this.hangingIndentLevel;
        const originalLevel = this.indentationLevel;
        this.hangingIndentLevel = this.indentationLevel + 1;
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
            hangingIndentLevel: this.hangingIndentLevel
        };
        this.write(" ");
    }

    markSpaceToConvertToNewLineIfHanging() {
        const index = this.items.length - 1;
        if (this.items[index] !== " ")
            throwError(`Expected the index at ${index} to be a space.`);
        this.spaceIndexesToConvertToNewLineOnHanging.push(index);
    }

    getIndentationLevel() {
        return this.indentationLevel;
    }

    /** Gets the zero-indexed line column. */
    getLineColumn() {
        if (this.currentLineColumn === 0)
            return this.indentationText.length;
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
        if (this.indentationLevel === level)
            return;

        this.indentationLevel = level;
        this.indentationText = this.singleIndentationText.repeat(level);
    }
}
