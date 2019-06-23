import { PrintItem, Group, PrintItemKind, Behaviour, Condition, Unknown, PrintItemIterator, Info, WriterInfo } from "../types";
import { assertNever, throwError, ResetableIterable } from "../utils";
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
    depth: number;
    minDepthFound: number;
    writerState: WriterState;
    uncommittedItems: PrintItem[];
}

class Printer {
    private readonly writer: Writer;
    private savePoint: SavePoint | undefined;
    private exitSymbol = Symbol("Thrown to exit when inside a group.");

    private depth = 0;

    constructor(private readonly options: PrintOptions) {
        this.writer = new Writer(options);
        this.writer.onNewLine(() => {
            this.commit();
            this.savePoint = undefined;
        });
    }

    commit() {
        this.writer.commit();
    }

    createSavePointIfAble() {
        if (this.savePoint != null && this.depth > this.savePoint.depth)
            return;

        this.savePoint = {
            depth: this.depth,
            minDepthFound: this.depth,
            writerState: this.writer.getState(),
            uncommittedItems: []
        };
    }

    revertToSavePointThrowingIfInGroup() {
        if (this.savePoint == null)
            return;

        if (this.depth > this.savePoint.depth)
            throw this.exitSymbol;

        this.updateStateToSavePoint();
    }

    updateStateToSavePoint() {
        if (this.savePoint == null)
            return;

        const savePoint = this.savePoint;
        this.savePoint = undefined;

        if (this.depth > savePoint.depth)
            throwError(`For some reason the group depth (${this.depth}) was greater than the save point group depth (${savePoint.depth}).`);

        this.writer.setState(savePoint.writerState);

        // reverting to save point means it should write a newline
        this.writer.write(this.options.newLineKind);

        const { uncommittedItems } = savePoint;
        for (let i = 1; i < uncommittedItems.length; i++)
            this.printPrintItem(uncommittedItems[i]);
    }

    printPrintItem(printItem: PrintItem) {
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
    }

    printBehaviour(behaviour: Behaviour) {
        if (behaviour === Behaviour.ExpectNewLine) {
            this.addToUncommittedItemsIfNecessary(behaviour);
            this.writer.markExpectNewLine();
        }
        else if (behaviour === Behaviour.NewLine) {
            this.createSavePointIfAble();
            this.addToUncommittedItemsIfNecessary(behaviour);
        }
        else if (behaviour === Behaviour.SpaceOrNewLine) {
            if (this.isAboveMaxWidth(1)) {
                const saveState = this.savePoint;
                if (saveState == null || saveState.depth >= this.depth)
                    this.writer.write(this.options.newLineKind);
                else {
                    this.addToUncommittedItemsIfNecessary(behaviour);
                    this.revertToSavePointThrowingIfInGroup();
                }
            }
            else {
                this.createSavePointIfAble();
                this.addToUncommittedItemsIfNecessary(behaviour);
                this.writer.write(" ");
            }
        }
        else if (behaviour === Behaviour.StartIndent) {
            this.addToUncommittedItemsIfNecessary(behaviour);
            this.writer.startIndent();
        }
        else if (behaviour === Behaviour.FinishIndent) {
            this.addToUncommittedItemsIfNecessary(behaviour);
            this.writer.finishIndent();
        }
        else if (behaviour === Behaviour.StartHangingIndent) {
            this.addToUncommittedItemsIfNecessary(behaviour);
            this.writer.startHangingIndent();
        }
        else if (behaviour === Behaviour.FinishHangingIndent) {
            this.addToUncommittedItemsIfNecessary(behaviour);
            this.writer.finishHangingIndent();
        }
        else {
            assertNever(behaviour);
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
        this.doUpdatingDepth(() => {
            if (group.items instanceof ResetableIterable)
                group.items.reset();
            else if (this.savePoint != null)
                group.items = new ResetableIterable(group.items);

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
        for (const item of items) {
            try {
                this.printPrintItem(item);
            } catch (err) {
                if (err !== this.exitSymbol || this.savePoint == null || this.depth !== this.savePoint.depth)
                    throw err;
                this.updateStateToSavePoint();
            }
        }
    }

    printUnknown(unknown: Unknown) {
        this.addToUncommittedItemsIfNecessary(unknown);
        this.writer.baseWrite(unknown.text);
    }

    private readonly resolvedConditions = new Map<Condition, boolean>();
    printCondition(condition: Condition) {
        this.addToUncommittedItemsIfNecessary(condition);
        this.doUpdatingDepth(() => {
            const conditionValue = this.getConditionValue(condition);
            if (conditionValue) {
                if (condition.true) {
                    if (condition.true instanceof ResetableIterable)
                        condition.true.reset();
                    else if (this.savePoint != null)
                        condition.true = new ResetableIterable(condition.true);

                    this.printItems(condition.true);
                }
            }
            else {
                if (condition.false) {
                    if (condition.false instanceof ResetableIterable)
                        condition.false.reset();
                    else if (this.savePoint != null)
                        condition.false = new ResetableIterable(condition.false);

                    this.printItems(condition.false);
                }
            }
        });
    }

    private getConditionValue(condition: Condition): boolean {
        if (typeof condition.condition === "object") {
            const result = this.resolvedConditions.get(condition.condition);
            if (result == null)
                return throwError(`Parser error: Cannot reference conditions that have not been printed first. ${JSON.stringify(condition)}`);
            return result;
        }
        else if (condition.condition instanceof Function) {
            const result = condition.condition({
                isConditionTrue: (c) => this.getConditionValue(c),
                writerInfo: this.getWriterInfo(),
                getResolvedInfo: info => this.getResolvedInfo(info)
            });
            this.resolvedConditions.set(condition, result);
            return result;
        }
        else {
            return assertNever(condition.condition);
        }
    }

    private readonly resolvedInfos = new Map<Info, WriterInfo>();
    resolveInfo(info: Info) {
        this.addToUncommittedItemsIfNecessary(info);
        this.resolvedInfos.set(info, this.getWriterInfo());
    }

    private getResolvedInfo(info: Info) {
        const resolvedInfo = this.resolvedInfos.get(info);
        if (resolvedInfo == null)
            return throwError(`Parser error: Cannot get the resolved info for an info that was not in the tree first.`);
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
        this.writer.commit();
        return this.writer.toString();
    }

    private addToUncommittedItemsIfNecessary(printItem: PrintItem) {
        if (this.savePoint == null || this.depth > this.savePoint.minDepthFound)
            return;

        // Add all the items at the top of the tree to the uncommitted items.
        // Their children will be iterated over later.
        this.savePoint.minDepthFound = this.depth;
        this.savePoint.uncommittedItems.push(printItem);
    }

    private isAboveMaxWidth(offset = 0) {
        // +1 to make the column 1-indexed
        return (this.writer.getLineColumn() + 1 + offset) > this.options.maxWidth;
    }
}
