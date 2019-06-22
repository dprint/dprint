import { PrintItem, Group, GroupSeparatorKind, PrintItemKind, Separator, Condition, Unknown, PrintItemIterator } from "../types";
import { assertNever, throwError, isIterator, ResetableIterable } from "../utils";
import { Writer, WriterState } from "./Writer";

export interface PrintOptions {
    maxWidth: number;
    indentSize: number; // todo: support tabs
    newLineKind: "\r\n" | "\n";
}

export function print(group: Group, options: PrintOptions) {
    const printer = new Printer({
        indentSize: options.indentSize,
        maxWidth: options.maxWidth,
        newLineKind: options.newLineKind
    });

    printer.printPrintItem(group);

    return printer.toString();
}

interface SavePoint {
    state: PrinterState;
    writerState: WriterState;
    uncommittedItems: PrintItem[];
}

interface PrinterState {
    groupDepth: number;
    groupStartLineNumber: number;
    groupIndentationLevel: number;
}

class Printer {
    private readonly writer: Writer;
    private savePoint: SavePoint | undefined;

    private state: PrinterState;

    constructor(private readonly options: PrintOptions) {
        this.writer = new Writer(options);
        this.writer.onNewLine(() => {
            this.commit();
            this.savePoint = undefined;
        });
        this.state = {
            groupDepth: 0,
            groupStartLineNumber: 0,
            groupIndentationLevel: 0
        };
    }

    getState(): Readonly<PrinterState> {
        return Printer.cloneState(this.state);
    }

    setState(state: Readonly<PrinterState>) {
        this.state = Printer.cloneState(state);
    }

    private static cloneState(printerState: Readonly<PrinterState>): PrinterState {
        const state: MakeRequired<PrinterState> = {
            groupDepth: printerState.groupDepth,
            groupIndentationLevel: printerState.groupIndentationLevel,
            groupStartLineNumber: printerState.groupStartLineNumber
        };
        return state;
    }

    commit() {
        this.writer.commit();
    }

    createSavePointIfAble() {
        if (this.savePoint != null && this.state.groupDepth > this.savePoint.state.groupDepth)
            return;

        this.savePoint = {
            state: this.getState(),
            writerState: this.writer.getState(),
            uncommittedItems: []
        };
    }

    revertToSavePointThrowingIfInGroup() {
        if (this.savePoint == null)
            return;

        if (this.savePoint.state.groupDepth > this.state.groupDepth)
            throw "exit";

        this.updateStateToSavePoint();
    }

    updateStateToSavePoint() {
        if (this.savePoint == null)
            return;

        const savePoint = this.savePoint;
        this.savePoint = undefined;
        this.setState(savePoint.state);
        this.writer.setState(savePoint.writerState);

        // reverting to save point means it should write a newline
        this.writer.write(this.options.newLineKind);

        const { uncommittedItems } = savePoint;
        for (let i = 1; i < uncommittedItems.length; i++)
            this.printPrintItem(uncommittedItems[i]);
    }

    printPrintItem(printItem: PrintItem) {
        if (typeof printItem === "number")
            this.printSeparator(printItem);
        else if (typeof printItem === "string")
            this.printString(printItem);
        else if (printItem.kind === PrintItemKind.Group)
            this.printGroup(printItem);
        else if (printItem.kind === PrintItemKind.Unknown)
            this.printUnknown(printItem);
        else {
            // todo: support conditionals
            //assertNever(printItem);
        }
    }

    printSeparator(separator: Separator) {
        if (separator === Separator.ExpectNewLine) {
            this.addToUncommittedItemsIfNecessary(separator);
            this.writer.markExpectNewLine();
        }
        else if (separator === Separator.NewLine) {
            this.createSavePointIfAble();
            this.addToUncommittedItemsIfNecessary(separator);
        }
        else if (separator === Separator.SpaceOrNewLine) {
            if (this.isAboveMaxWidth(1)) {
                const saveState = this.savePoint;
                if (saveState == null || saveState.state.groupDepth >= this.state.groupDepth)
                    this.writer.write(this.options.newLineKind);
                else {
                    this.addToUncommittedItemsIfNecessary(separator);
                    this.revertToSavePointThrowingIfInGroup();
                }
            }
            else {
                this.createSavePointIfAble();
                this.addToUncommittedItemsIfNecessary(separator);
                this.writer.write(" ");
            }
        }
    }

    printString(text: string) {
        // todo: this check should only happen during testing
        const isNewLine = text === "\n" || text === "\r\n";
        if (!isNewLine && text.includes("\n"))
            throw new Error("Praser error: Cannot parse text that includes newlines. Newlines must be in their own string.");

        this.addToUncommittedItemsIfNecessary(text);
        if (!isNewLine && this.savePoint != null && this.isAboveMaxWidth(text.length))
            this.revertToSavePointThrowingIfInGroup();
        else
            this.writer.write(text);
    }

    printGroup(group: Group) {
        this.addToUncommittedItemsIfNecessary(group);
        const previousState = this.getState();

        this.state = {
            groupStartLineNumber: this.writer.getLineNumber(),
            groupIndentationLevel: this.writer.getIndentationLevel(),
            groupDepth: previousState.groupDepth + 1
        };

        if (group.items instanceof ResetableIterable)
            group.items.reset();
        else if (this.savePoint != null)
            group.items = new ResetableIterable(group.items);

        if (group.hangingIndent)
            this.writer.hangingIndent(() => this.printItems(group, this.state.groupDepth));
        else if (group.indent)
            this.writer.indent(() => this.printItems(group, this.state.groupDepth));
        else
            this.printItems(group, this.state.groupDepth);

        this.setState(previousState);
    }

    private printItems(group: Group, groupDepth: number) {
        for (const item of group.items) {
            try {
                this.printPrintItem(item);
            } catch (err) {
                if (err !== "exit" || this.state.groupDepth !== groupDepth)
                    throw err;
                this.updateStateToSavePoint();
            }
        }
    }

    printUnknown(unknown: Unknown) {
        this.addToUncommittedItemsIfNecessary(unknown);
        this.writer.baseWrite(unknown.text);
    }

    toString() {
        this.writer.commit();
        return this.writer.toString();
    }

    private addToUncommittedItemsIfNecessary(printItem: PrintItem) {
        if (this.savePoint != null && this.savePoint.state.groupDepth === this.state.groupDepth)
            this.savePoint.uncommittedItems.push(printItem);
    }

    private isAboveMaxWidth(offset = 0) {
        return (this.writer.getLineColumn() + offset) > this.options.maxWidth;
    }
}