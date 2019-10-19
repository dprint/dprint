import { PrintItemKind, Signal, Condition, RawString, PrintItemIterable, Info, WriterInfo } from "../types";
import { assertNever } from "../utils";
import { Writer, WriterState } from "./Writer";
import { PrinterPrintItem, ConditionContainer, PrintItemContainer } from "./types";
import { deepIterableToContainer } from "./utils";

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
    readonly name: string;
    readonly newlineGroupDepth: number;
    readonly container: PrintItemContainer;
    readonly currentIndexes: number[];
    readonly writerState: Readonly<WriterState>;
    readonly possibleNewLineSavePoint: SavePoint | undefined;
}

const exitSymbol = Symbol("Used in certain cases when a save point is restored.");

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
    let lastLog: string | undefined;

    // save point items
    let possibleNewLineSavePoint: SavePoint | undefined;
    let newlineGroupDepth = 0;
    let currentIndexes = [0];
    let container = deepIterableToContainer(iterable);

    writer.onNewLine(() => {
        possibleNewLineSavePoint = undefined;
    });

    printItems();

    return writer.toString();

    function printItems() {
        while (true) {
            while (currentIndexes[currentIndexes.length - 1] < container.items.length) {
                handlePrintItem(container.items[currentIndexes[currentIndexes.length - 1]]);
                currentIndexes[currentIndexes.length - 1]++;
            }

            if (container.parent == null)
                return;

            container = container.parent;
            currentIndexes.pop();
            currentIndexes[currentIndexes.length - 1]++;
        }
    }

    function handlePrintItem(printItem: PrinterPrintItem) {
        if (typeof printItem === "number")
            handleSignal(printItem);
        else if (typeof printItem === "string")
            handleString(printItem);
        else if (printItem.kind === PrintItemKind.RawString)
            handleRawString(printItem);
        else if (printItem.kind === PrintItemKind.Condition)
            handleCondition(printItem);
        else if (printItem.kind === PrintItemKind.Info)
            resolveInfo(printItem);
        else
            assertNever(printItem);

        // logWriterForDebugging();

        function handleSignal(signal: Signal) {
            switch (signal) {
                case Signal.NewLine:
                    writer.write(options.newlineKind);
                    break;
                case Signal.ExpectNewLine:
                    writer.markExpectNewLine();
                    break;
                case Signal.PossibleNewLine:
                    markPossibleNewLineIfAble();
                    break;
                case Signal.SpaceOrNewLine:
                    if (isAboveMaxWidth(1)) {
                        const saveState = possibleNewLineSavePoint;
                        if (saveState == null || saveState.newlineGroupDepth >= newlineGroupDepth)
                            writer.write(options.newlineKind);
                        else if (possibleNewLineSavePoint != null)
                            updateStateToSavePoint(possibleNewLineSavePoint);
                    }
                    else {
                        markPossibleNewLineIfAble();
                        writer.write(" ");
                    }
                    break;
                case Signal.StartIndent:
                    writer.startIndent();
                    break;
                case Signal.FinishIndent:
                    writer.finishIndent();
                    break;
                case Signal.StartNewLineGroup:
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

        function handleString(text: string) {
            // todo: this check should only happen during testing
            const isNewLine = text === "\n" || text === "\r\n";
            if (!isNewLine && text.includes("\n"))
                throw new Error("Parser error: Cannot parse text that includes newlines. Newlines must be in their own string.");

            if (!isNewLine && possibleNewLineSavePoint != null && isAboveMaxWidth(text.length))
                updateStateToSavePoint(possibleNewLineSavePoint);
            else
                writer.write(text);
        }

        function handleRawString(unknown: RawString) {
            if (possibleNewLineSavePoint != null && isAboveMaxWidth(getLineWidth()))
                updateStateToSavePoint(possibleNewLineSavePoint);
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

        function handleCondition(condition: ConditionContainer) {
            try {
                const conditionValue = getConditionValue(condition.originalCondition, condition.originalCondition);
                if (conditionValue) {
                    if (condition.true) {
                        container = condition.true;
                        currentIndexes.push(-1);
                    }
                }
                else {
                    if (condition.false) {
                        container = condition.false;
                        currentIndexes.push(-1);
                    }
                }
            } catch (err) {
                if (err !== exitSymbol)
                    throw err;
            }
        }
    }

    function markPossibleNewLineIfAble() {
        if (possibleNewLineSavePoint != null && newlineGroupDepth > possibleNewLineSavePoint.newlineGroupDepth)
            return;

        possibleNewLineSavePoint = createSavePoint("newline");
    }

    function updateStateToSavePoint(savePoint: SavePoint) {
        const isForNewLine = possibleNewLineSavePoint === savePoint;
        writer.setState(savePoint.writerState);
        possibleNewLineSavePoint = isForNewLine ? undefined : savePoint.possibleNewLineSavePoint;
        container = savePoint.container;
        currentIndexes = [...savePoint.currentIndexes]; // todo: probably doesn't need to be cloned
        newlineGroupDepth = savePoint.newlineGroupDepth;

        if (isForNewLine)
            writer.write(options.newlineKind);
    }

    function getConditionValue(condition: Condition, printingCondition: Condition): boolean | undefined {
        if (typeof condition.condition === "object") {
            const result = resolvedConditions.get(condition.condition);

            if (result == null) {
                if (!lookAheadSavePoints.has(condition)) {
                    const savePoint = createSavePointForRestoringCondition(condition.name);
                    lookAheadSavePoints.set(condition, savePoint);
                }
            }
            else {
                const savePoint = lookAheadSavePoints.get(condition);
                if (savePoint != null) {
                    lookAheadSavePoints.delete(condition);
                    updateStateToSavePoint(savePoint);
                    throw exitSymbol;
                }
            }

            return result;
        }
        else if (condition.condition instanceof Function) {
            const result = condition.condition({
                getResolvedCondition,
                writerInfo: getWriterInfo(),
                getResolvedInfo: info => getResolvedInfo(info)
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
            const conditionValue = getConditionValue(c, printingCondition);
            if (conditionValue == null)
                return defaultValue;
            return conditionValue;
        }

        function getResolvedInfo(info: Info) {
            const resolvedInfo = resolvedInfos.get(info);
            if (resolvedInfo == null && !lookAheadSavePoints.has(info)) {
                const savePoint = createSavePointForRestoringCondition(info.name);
                lookAheadSavePoints.set(info, savePoint);
            }
            return resolvedInfo;
        }
    }

    function resolveInfo(info: Info) {
        resolvedInfos.set(info, getWriterInfo());

        const savePoint = lookAheadSavePoints.get(info);
        if (savePoint != null) {
            lookAheadSavePoints.delete(info);
            updateStateToSavePoint(savePoint);
        }
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

    function isAboveMaxWidth(offset = 0) {
        // +1 to make the column 1-indexed
        return (writer.getLineColumn() + 1 + offset) > options.maxWidth;
    }

    function createSavePointForRestoringCondition(conditionName: string): SavePoint {
        const savePoint = createSavePoint(conditionName);
        // decrement the last child index so it repeats the condition
        savePoint.currentIndexes[savePoint.currentIndexes.length - 1]--;
        return savePoint;
    }

    function createSavePoint(name: string): SavePoint {
        return {
            name,
            currentIndexes: [...currentIndexes],
            newlineGroupDepth,
            writerState: writer.getState(),
            possibleNewLineSavePoint,
            container
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
