import { PrintItem, PrintItemKind, Signal, Condition, RawString, PrintItemIterable, Info, WriterInfo } from "../types";
import { assertNever, RepeatableIterable } from "../utils";
import { Writer, WriterState } from "./Writer";

// todo: for performance reasons, when doing look aheads, it should only leap back if the condition changes

/** Options for printing a print item iterable. */
export interface PrintOptions {
    /** The width the printer will attempt to keep the line under. */
    maxWidth: number;
    /** The number of spaces to use when indenting (unless useTabs is true). */
    indentWidth: number;
    /** Whether to use tabs for indenting. */
    useTabs: boolean;
    /** The newline character to use when doing a new line. */
    newlineKind: "\r\n" | "\n";
}

interface SavePoint {
    /** Name for debugging purposes. */
    name?: string;
    newlineGroupDepth: number;
    childIndex: number;
    writerState: WriterState;
    possibleNewLineSavePoint: SavePoint | undefined;

    minDepthFound: number;
    minDepthChildIndex: number;
    uncomittedItems: PrintItem[];
}

const exitSymbol = Symbol("Thrown to exit when down a depth.");

// todo: separate out more of this code (ex. resolving conditions, infos, and dealing with save points could be in separate classes)

/**
 * Prints out the provided print item iterable.
 * @param iterable - Iterable to iterate and print.
 * @param options - Options for printing.
 */
export function print(iterable: PrintItemIterable, options: PrintOptions) {
    // setup
    const writer = new Writer(options);
    const resolvedConditions = new Map<Condition, boolean>();
    const resolvedInfos = new Map<Info, WriterInfo>();
    const lookAheadSavePoints = new Map<Condition | Info, SavePoint>();
    let possibleNewLineSavePoint: SavePoint | undefined;
    let depth = 0;
    let childIndex = 0;
    let newlineGroupDepth = 0;
    let savePointToResume: SavePoint | undefined;
    let lastLog: string | undefined;

    writer.onNewLine(() => {
        possibleNewLineSavePoint = undefined;
    });

    // print and get final string
    printItems(iterable);

    return writer.toString();

    function printItems(items: PrintItemIterable) {
        childIndex = 0;

        for (const item of items) {
            const previousChildIndex = childIndex;

            printPrintItem(item);

            childIndex = previousChildIndex + 1;
        }
    }

    function printPrintItem(printItem: PrintItem) {
        try {
            printInternal();
        } catch (err) {
            if (err !== exitSymbol || savePointToResume == null || depth !== savePointToResume.minDepthFound)
                throw err;
            updateStateToSavePoint(savePointToResume);
        }

        function printInternal() {
            addToUncommittedItemsIfNecessary(printItem);

            if (typeof printItem === "number")
                printSignal(printItem);
            else if (typeof printItem === "string")
                printString(printItem);
            else if (printItem.kind === PrintItemKind.RawString)
                printRawString(printItem);
            else if (printItem.kind === PrintItemKind.Condition)
                printCondition(printItem);
            else if (printItem.kind === PrintItemKind.Info)
                resolveInfo(printItem);
            else
                assertNever(printItem);

            // logWriterForDebugging();
        }

        function printSignal(signal: Signal) {
            switch (signal) {
                case Signal.ExpectNewLine:
                    writer.markExpectNewLine();
                    break;
                case Signal.NewLine:
                    markPossibleNewLineIfAble(signal);
                    break;
                case Signal.SpaceOrNewLine:
                    if (isAboveMaxWidth(1)) {
                        const saveState = possibleNewLineSavePoint;
                        if (saveState == null || saveState.newlineGroupDepth >= newlineGroupDepth)
                            writer.write(options.newlineKind);
                        else {
                            if (possibleNewLineSavePoint != null)
                                revertToSavePointPossiblyThrowing(possibleNewLineSavePoint);
                        }
                    }
                    else {
                        markPossibleNewLineIfAble(signal);
                        writer.write(" ");
                    }
                    break;
                case Signal.StartIndent:
                    writer.startIndent();
                    break;
                case Signal.FinishIndent:
                    writer.finishIndent();
                    break;
                case Signal.StartNewlineGroup:
                    newlineGroupDepth++;
                    break;
                case Signal.FinishNewLineGroup:
                    newlineGroupDepth--;
                    break;
                case Signal.SingleIndent:
                    writer.singleIndent();
                    break;
                case Signal.StartIgnoringIndent:
                    writer.startIgnoringIndent();
                    break;
                case Signal.FinishIgnoringIndent:
                    writer.finishIgnoringIndent();
                    break;
                default:
                    assertNever(signal);
                    break;
            }
        }

        function printString(text: string) {
            // todo: this check should only happen during testing
            const isNewLine = text === "\n" || text === "\r\n";
            if (!isNewLine && text.includes("\n"))
                throw new Error("Praser error: Cannot parse text that includes newlines. Newlines must be in their own string.");

            if (!isNewLine && possibleNewLineSavePoint != null && isAboveMaxWidth(text.length))
                revertToSavePointPossiblyThrowing(possibleNewLineSavePoint);
            else
                writer.write(text);
        }

        function printRawString(unknown: RawString) {
            if (possibleNewLineSavePoint != null && isAboveMaxWidth(getLineWidth()))
                revertToSavePointPossiblyThrowing(possibleNewLineSavePoint);
            else
                writer.baseWrite(unknown.text);

            function getLineWidth() {
                const index = unknown.text.indexOf("\n");
                if (index === -1)
                    return unknown.text.length;
                else if (unknown.text[index - 1] === "\r")
                    return index - 1;
                return index;
            }
        }

        function printCondition(condition: Condition) {
            const conditionValue = getConditionValue(condition);
            doUpdatingDepth(() => {
                if (conditionValue) {
                    if (condition.true) {
                        const isRepeatableIterable = condition.true instanceof RepeatableIterable;
                        if (!isRepeatableIterable && hasUncomittedItems())
                            condition.true = new RepeatableIterable(condition.true);

                        printItems(condition.true);
                    }
                }
                else {
                    if (condition.false) {
                        const isRepeatableIterable = condition.false instanceof RepeatableIterable;
                        if (!isRepeatableIterable && hasUncomittedItems())
                            condition.false = new RepeatableIterable(condition.false);

                        printItems(condition.false);
                    }
                }
            });
        }
    }

    function markPossibleNewLineIfAble(signal: Signal) {
        if (possibleNewLineSavePoint != null && newlineGroupDepth > possibleNewLineSavePoint.newlineGroupDepth)
            return;

        possibleNewLineSavePoint = createSavePoint(signal);
    }

    function revertToSavePointPossiblyThrowing(savePoint: SavePoint) {
        if (depth === savePoint.minDepthFound) {
            updateStateToSavePoint(savePoint);
            return;
        }

        savePointToResume = savePoint;
        throw exitSymbol;
    }

    function addToUncommittedItemsIfNecessary(printItem: PrintItem) {
        if (possibleNewLineSavePoint != null)
            updateSavePoint(possibleNewLineSavePoint);
        for (const savePoint of lookAheadSavePoints.values())
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

    function updateStateToSavePoint(savePoint: SavePoint) {
        const isForNewLine = possibleNewLineSavePoint === savePoint;
        writer.setState(savePoint.writerState);
        possibleNewLineSavePoint = isForNewLine ? undefined : savePoint.possibleNewLineSavePoint;
        childIndex = savePoint.childIndex;
        newlineGroupDepth = savePoint.newlineGroupDepth;

        if (isForNewLine)
            writer.write(options.newlineKind);

        const startIndex = isForNewLine ? 1 : 0;
        childIndex += startIndex;
        for (let i = startIndex; i < savePoint.uncomittedItems.length; i++) {
            const previousChildIndex = childIndex;

            printPrintItem(savePoint.uncomittedItems[i]);

            childIndex = previousChildIndex + 1;
        }
    }

    function getConditionValue(condition: Condition): boolean | undefined {
        if (typeof condition.condition === "object") {
            const result = resolvedConditions.get(condition.condition);

            if (result == null) {
                if (!lookAheadSavePoints.has(condition)) {
                    const savePoint = createSavePoint(condition);
                    savePoint.name = condition.name;
                    lookAheadSavePoints.set(condition, savePoint);
                }
            }
            else {
                const savePoint = lookAheadSavePoints.get(condition);
                if (savePoint != null) {
                    lookAheadSavePoints.delete(condition);
                    revertToSavePointPossiblyThrowing(savePoint);
                }
            }

            return result;
        }
        else if (condition.condition instanceof Function) {
            const result = condition.condition({
                getResolvedCondition,
                writerInfo: getWriterInfo(),
                getResolvedInfo: info => getResolvedInfo(info, condition)
            });
            if (result != null)
                resolvedConditions.set(condition, result);
            return result;
        }
        else {
            return assertNever(condition.condition);
        }

        function getResolvedCondition(c: Condition): boolean | undefined;
        function getResolvedCondition(c: Condition, defaultValue: boolean): boolean;
        function getResolvedCondition(c: Condition, defaultValue?: boolean): boolean | undefined {
            const conditionValue = getConditionValue(c);
            if (conditionValue == null)
                return defaultValue;
            return conditionValue;
        }
    }

    function resolveInfo(info: Info) {
        resolvedInfos.set(info, getWriterInfo());

        const savePoint = lookAheadSavePoints.get(info);
        if (savePoint != null) {
            lookAheadSavePoints.delete(info);
            revertToSavePointPossiblyThrowing(savePoint);
        }
    }

    function getResolvedInfo(info: Info, parentCondition: Condition) {
        const resolvedInfo = resolvedInfos.get(info);
        if (resolvedInfo == null && !lookAheadSavePoints.has(info)) {
            const savePoint = createSavePoint(parentCondition);
            savePoint.name = info.name;
            lookAheadSavePoints.set(info, savePoint);
        }
        return resolvedInfo;
    }

    function getWriterInfo(): WriterInfo {
        return {
            lineStartIndentLevel: writer.getLineStartIndentLevel(),
            lineStartColumnNumber: writer.getLineStartColumnNumber(),
            lineNumber: writer.getLineNumber(),
            columnNumber: writer.getLineColumn(),
            indentLevel: writer.getIndentationLevel()
        };
    }

    function doUpdatingDepth(action: () => void) {
        const previousDepth = depth;
        depth++;

        try {
            action();
        } finally {
            depth = previousDepth;
        }
    }

    function hasUncomittedItems() {
        return possibleNewLineSavePoint != null || lookAheadSavePoints.size > 0;
    }

    function isAboveMaxWidth(offset = 0) {
        // +1 to make the column 1-indexed
        return (writer.getLineColumn() + 1 + offset) > options.maxWidth;
    }

    function createSavePoint(initialItem: PrintItem): SavePoint {
        return {
            childIndex,
            newlineGroupDepth,
            writerState: writer.getState(),
            possibleNewLineSavePoint,
            uncomittedItems: [initialItem],
            minDepthFound: depth,
            minDepthChildIndex: childIndex
        };
    }

    function logWriterForDebugging() {
        const currentText = writer.toString();
        if (lastLog === currentText)
            return;

        lastLog = currentText;
        console.log("----");
        console.log(currentText);
    }
}
