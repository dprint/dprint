import { PrintItem, Group, PrintItemKind, Behaviour, Condition, Unknown, PrintItemIterator, Info, WriterInfo } from "../types";
import { assertNever, RepeatableIterator } from "../utils";
import { Writer, WriterState } from "./Writer";

// todo: for performance reasons, when doing look aheads, it should only leap back if the condition changes

export interface PrintOptions {
    maxWidth: number;
    indentSize: number;
    useTabs: boolean;
    newLineKind: "\r\n" | "\n";
}

export function print(group: Group, options: PrintOptions) {
    const printer = new Printer(options);

    printer.printPrintItem(group);

    return printer.toString();
}

interface SavePoint {
    /** Name for debugging purposes. */
    name?: string;
    depth: number;
    childIndex: number;
    writerState: WriterState;
    possibleNewLineSavePoint: SavePoint | undefined;

    minDepthFound: number;
    minDepthChildIndex: number;
    uncomittedItems: PrintItem[];
}

// todo: probably change this to functions rather than a class...
class Printer {
    private readonly writer: Writer;
    private possibleNewLineSavePoint: SavePoint | undefined;
    private lookAheadSavePoints = new Map<Condition | Info, SavePoint>();

    private exitSymbol = Symbol("Thrown to exit when inside a group.");

    private depth = 0;
    private childIndex = 0;

    constructor(private readonly options: PrintOptions) {
        this.writer = new Writer(options);
        this.writer.onNewLine(() => {
            this.possibleNewLineSavePoint = undefined;
        });
    }

    markPossibleNewLineIfAble(behaviour: Behaviour) {
        if (this.possibleNewLineSavePoint != null && this.depth > this.possibleNewLineSavePoint.depth)
            return;

        this.possibleNewLineSavePoint = this.createSavePoint(behaviour);
    }

    private createSavePoint(initialItem: PrintItem): SavePoint {
        return {
            depth: this.depth,
            childIndex: this.childIndex,
            writerState: this.writer.getState(),
            possibleNewLineSavePoint: this.possibleNewLineSavePoint,
            uncomittedItems: [initialItem],
            minDepthFound: this.depth,
            minDepthChildIndex: this.childIndex
        };
    }

    private savePointToResume: SavePoint | undefined;
    revertToSavePointThrowing(savePoint: SavePoint) {
        this.savePointToResume = savePoint;
        throw this.exitSymbol;
    }

    printPrintItem(printItem: PrintItem) {
        this.addToUncommittedItemsIfNecessary(printItem);

        // todo: nest all these function within printPrintItem to prevent
        // them from being used elsewhere
        if (typeof printItem === "number")
            this.printBehaviour(printItem);
        else if (typeof printItem === "string")
            this.printString(printItem);
        else if (printItem.kind === PrintItemKind.Group)
            this.printGroup(printItem);
        else if (printItem.kind === PrintItemKind.Unknown)
            this.printUnknown(printItem);
        else if (printItem.kind === PrintItemKind.Condition)
            this.printCondition(printItem);
        else if (printItem.kind === PrintItemKind.Info)
            this.resolveInfo(printItem);
        else
            assertNever(printItem);

        //this.logWriterForDebugging();
    }

    private lastLog: string = "";
    private logWriterForDebugging() {
        const currentText = this.writer.toString();
        if (this.lastLog !== currentText) {
            this.lastLog = currentText;
            console.log("----");
            console.log(currentText);
        }
    }

    printBehaviour(behaviour: Behaviour) {
        if (behaviour === Behaviour.ExpectNewLine)
            this.writer.markExpectNewLine();
        else if (behaviour === Behaviour.NewLine)
            this.markPossibleNewLineIfAble(behaviour);
        else if (behaviour === Behaviour.SpaceOrNewLine) {
            if (this.isAboveMaxWidth(1)) {
                const saveState = this.possibleNewLineSavePoint;
                if (saveState == null || saveState.depth >= this.depth)
                    this.writer.write(this.options.newLineKind);
                else {
                    if (this.possibleNewLineSavePoint != null)
                        this.revertToSavePointThrowing(this.possibleNewLineSavePoint);
                }
            }
            else {
                this.markPossibleNewLineIfAble(behaviour);
                this.writer.write(" ");
            }
        }
        else if (behaviour === Behaviour.StartIndent)
            this.writer.startIndent();
        else if (behaviour === Behaviour.FinishIndent)
            this.writer.finishIndent();
        else if (behaviour === Behaviour.StartHangingIndent)
            this.writer.startHangingIndent();
        else if (behaviour === Behaviour.FinishHangingIndent)
            this.writer.finishHangingIndent();
        else
            assertNever(behaviour);
    }

    printString(text: string) {
        // todo: this check should only happen during testing
        const isNewLine = text === "\n" || text === "\r\n";
        if (!isNewLine && text.includes("\n"))
            throw new Error("Praser error: Cannot parse text that includes newlines. Newlines must be in their own string.");

        if (!isNewLine && this.possibleNewLineSavePoint != null && this.isAboveMaxWidth(text.length))
            this.revertToSavePointThrowing(this.possibleNewLineSavePoint);
        else
            this.writer.write(text);
    }

    printGroup(group: Group) {
        this.doUpdatingDepth(() => {
            const isRepeatableIterator = group.items instanceof RepeatableIterator;
            if (!isRepeatableIterator && this.hasUncomittedItems())
                group.items = new RepeatableIterator(group.items);

            this.printItems(group.items);
        });
    }

    private doUpdatingDepth(action: () => void) {
        const previousDepth = this.depth;
        this.depth++;

        try {
            action();
        } finally {
            this.depth = previousDepth;
        }
    }

    private printItems(items: PrintItemIterator) {
        this.childIndex = 0;

        for (const item of items) {
            const previousChildIndex = this.childIndex;
            try {
                this.printPrintItem(item);
            } catch (err) {
                const savePointToResume = this.savePointToResume;
                if (err !== this.exitSymbol || savePointToResume == null || this.depth !== savePointToResume.depth)
                    throw err;
                this.savePointToResume = undefined;
                this.updateStateToSavePoint(savePointToResume);
            }

            this.childIndex = previousChildIndex + 1;
        }
    }

    private updateStateToSavePoint(savePoint: SavePoint) {
        const isForNewLine = this.possibleNewLineSavePoint === savePoint;
        this.writer.setState(savePoint.writerState);
        this.possibleNewLineSavePoint = isForNewLine ? undefined : savePoint.possibleNewLineSavePoint;
        this.depth = savePoint.depth;
        this.childIndex = savePoint.childIndex;

        if (isForNewLine)
            this.writer.write(this.options.newLineKind);

        const startIndex = isForNewLine ? 1 : 0;
        this.childIndex += startIndex;
        for (let i = startIndex; i < savePoint.uncomittedItems.length; i++) {
            const previousChildIndex = this.childIndex;
            this.printPrintItem(savePoint.uncomittedItems[i]);
            this.childIndex = previousChildIndex + 1;
        }
    }

    printUnknown(unknown: Unknown) {
        this.writer.baseWrite(unknown.text);
    }

    private readonly resolvedConditions = new Map<Condition, boolean>();
    printCondition(condition: Condition) {
        const conditionValue = this.getConditionValue(condition);
        this.doUpdatingDepth(() => {
            if (conditionValue) {
                if (condition.true) {
                    const isRepeatableIterator = condition.true instanceof RepeatableIterator;
                    if (!isRepeatableIterator && this.hasUncomittedItems())
                        condition.true = new RepeatableIterator(condition.true);

                    this.printItems(condition.true);
                }
            }
            else {
                if (condition.false) {
                    const isRepeatableIterator = condition.false instanceof RepeatableIterator;
                    if (!isRepeatableIterator && this.hasUncomittedItems())
                        condition.false = new RepeatableIterator(condition.false);

                    this.printItems(condition.false);
                }
            }
        });
    }

    private getConditionValue(condition: Condition): boolean | undefined {
        const _this = this;
        if (typeof condition.condition === "object") {
            const result = this.resolvedConditions.get(condition.condition);

            if (result == null) {
                if (!this.lookAheadSavePoints.has(condition)) {
                    const savePoint = this.createSavePoint(condition);
                    savePoint.name = condition.name;
                    this.lookAheadSavePoints.set(condition, savePoint);
                }
            }
            else {
                const savePoint = this.lookAheadSavePoints.get(condition);
                if (savePoint != null) {
                    this.lookAheadSavePoints.delete(condition);
                    this.revertToSavePointThrowing(savePoint);
                }
            }

            return result;
        }
        else if (condition.condition instanceof Function) {
            const result = condition.condition({
                getResolvedCondition,
                writerInfo: this.getWriterInfo(),
                getResolvedInfo: info => this.getResolvedInfo(info, condition)
            });
            if (result != null)
                this.resolvedConditions.set(condition, result);
            return result;
        }
        else {
            return assertNever(condition.condition);
        }

        function getResolvedCondition(c: Condition): boolean | undefined;
        function getResolvedCondition(c: Condition, defaultValue: boolean): boolean;
        function getResolvedCondition(c: Condition, defaultValue?: boolean): boolean | undefined {
            const conditionValue = _this.getConditionValue(c);
            if (conditionValue == null)
                return defaultValue;
            return conditionValue;
        }
    }

    private readonly resolvedInfos = new Map<Info, WriterInfo>();
    resolveInfo(info: Info) {
        this.resolvedInfos.set(info, this.getWriterInfo());

        const savePoint = this.lookAheadSavePoints.get(info);
        if (savePoint != null) {
            this.lookAheadSavePoints.delete(info);
            this.revertToSavePointThrowing(savePoint);
        }
    }

    private getResolvedInfo(info: Info, parentCondition: Condition) {
        const resolvedInfo = this.resolvedInfos.get(info);
        if (resolvedInfo == null && !this.lookAheadSavePoints.has(info)) {
            const savePoint = this.createSavePoint(parentCondition);
            savePoint.name = info.name;
            this.lookAheadSavePoints.set(info, savePoint);
        }
        return resolvedInfo;
    }

    private getWriterInfo(): WriterInfo {
        return {
            lineStartIndentLevel: this.writer.getLineStartIndentLevel(),
            lineNumber: this.writer.getLineNumber(),
            columnNumber: this.writer.getLineColumn()
        };
    }

    toString() {
        //this.writer.commit();
        return this.writer.toString();
    }

    hasUncomittedItems() {
        return this.possibleNewLineSavePoint != null || this.lookAheadSavePoints.size > 0;
    }

    private addToUncommittedItemsIfNecessary(printItem: PrintItem) {
        const depth = this.depth;
        const childIndex = this.childIndex;

        if (this.possibleNewLineSavePoint != null)
            updateSavePoint(this.possibleNewLineSavePoint);
        for (const savePoint of this.lookAheadSavePoints.values())
            updateSavePoint(savePoint);

        function updateSavePoint(savePoint: SavePoint) {
            if (depth > savePoint.minDepthFound)
                return;

            // Add all the items at the top of the tree to the uncommitted items.
            // Their children will be iterated over later.
            if (depth < savePoint.minDepthFound) {
                savePoint.minDepthChildIndex = childIndex;
                savePoint.minDepthFound = depth;
                savePoint.uncomittedItems.push(printItem);
            }
            else if (childIndex > savePoint.minDepthChildIndex) {
                savePoint.minDepthChildIndex = childIndex;
                savePoint.uncomittedItems.push(printItem);
            }
        }
    }

    private isAboveMaxWidth(offset = 0) {
        // +1 to make the column 1-indexed
        return (this.writer.getLineColumn() + 1 + offset) > this.options.maxWidth;
    }
}
