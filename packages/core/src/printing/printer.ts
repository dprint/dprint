import { PrintItemKind, Signal, Condition, PrintItemIterable, Info, WriterInfo } from "@dprint/types";
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
    newLineKind: "\r\n" | "\n";
    /**
     * Set to true when testing in order to run additional validation on the inputted strings, which
     * ensures the printer is being used correctly.
     */
    isTesting: boolean;
}

interface SavePoint {
    /** Name for debugging purposes. */
    readonly name: string;
    readonly newLineGroupDepth: number;
    readonly container: PrintItemContainer;
    readonly currentIndexes: number[];
    readonly writerState: Readonly<WriterState>;
    readonly possibleNewLineSavePoint: SavePoint | undefined;
}

/**
 * Prints out the provided print item iterable.
 * @param iterable - Iterable to iterate and print.
 * @param options - Options for printing.
 */
export function print(iterable: PrintItemIterable, options: PrintOptions) {
    // setup
    const writer = new Writer(options);
    const resolvedConditions = new Map<Condition, boolean | undefined>();
    const resolvedInfos = new Map<Info, WriterInfo>();
    const lookAheadSavePoints = new Map<Condition | Info, SavePoint>();
    let lastLog: string | undefined;

    // save point items
    let possibleNewLineSavePoint: SavePoint | undefined;
    let newLineGroupDepth = 0;
    let currentIndexes = [0];
    let container = deepIterableToContainer(iterable);

    writer.onNewLine(() => {
        possibleNewLineSavePoint = undefined;
        refreshWriterUseCommittedItems();
    });

    printItems();

    return writer.toString();

    function refreshWriterUseCommittedItems() {
        writer.setUseCommittedItems(lookAheadSavePoints.size === 0 && possibleNewLineSavePoint == null);
    }

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
        else if (printItem.kind === PrintItemKind.Condition)
            handleCondition(printItem);
        else if (printItem.kind === PrintItemKind.Info)
            handleInfo(printItem);
        else
            assertNever(printItem);

        // logWriterForDebugging();

        function handleSignal(signal: Signal) {
            switch (signal) {
                case Signal.NewLine:
                    writer.newLine();
                    break;
                case Signal.Tab:
                    writer.tab();
                    break;
                case Signal.ExpectNewLine:
                    writer.markExpectNewLine();
                    break;
                case Signal.PossibleNewLine:
                    markPossibleNewLineIfAble();
                    break;
                case Signal.SpaceOrNewLine:
                    if (isAboveMaxWidth(1)) {
                        const savePoint = possibleNewLineSavePoint;
                        if (savePoint == null || savePoint.newLineGroupDepth >= newLineGroupDepth)
                            writer.newLine();
                        else if (savePoint != null)
                            updateStateToSavePoint(savePoint);
                    }
                    else {
                        markPossibleNewLineIfAble();
                        writer.space();
                    }
                    break;
                case Signal.StartIndent:
                    writer.startIndent();
                    break;
                case Signal.FinishIndent:
                    writer.finishIndent();
                    break;
                case Signal.StartNewLineGroup:
                    newLineGroupDepth++;
                    break;
                case Signal.FinishNewLineGroup:
                    newLineGroupDepth--;
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
            if (possibleNewLineSavePoint != null && isAboveMaxWidth(text.length))
                updateStateToSavePoint(possibleNewLineSavePoint);
            else
                writer.write(text);
        }

        function handleCondition(condition: ConditionContainer) {
            const conditionValue = getConditionValue(condition.originalCondition);
            resolvedConditions.set(condition.originalCondition, conditionValue);

            const savePoint = lookAheadSavePoints.get(condition.originalCondition);
            if (conditionValue != null && savePoint != null) {
                lookAheadSavePoints.delete(condition.originalCondition);
                updateStateToSavePoint(savePoint);
                return;
            }

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
        }
    }

    function markPossibleNewLineIfAble() {
        if (possibleNewLineSavePoint != null && newLineGroupDepth > possibleNewLineSavePoint.newLineGroupDepth)
            return;

        possibleNewLineSavePoint = createSavePoint("newline");
        refreshWriterUseCommittedItems();
    }

    function updateStateToSavePoint(savePoint: SavePoint) {
        const isForNewLine = possibleNewLineSavePoint === savePoint;
        writer.setState(savePoint.writerState);
        possibleNewLineSavePoint = isForNewLine ? undefined : savePoint.possibleNewLineSavePoint;
        container = savePoint.container;
        currentIndexes = [...savePoint.currentIndexes]; // todo: probably doesn't need to be cloned
        newLineGroupDepth = savePoint.newLineGroupDepth;

        if (isForNewLine)
            writer.newLine();

        refreshWriterUseCommittedItems();
    }

    function getConditionValue(printingCondition: Condition): boolean | undefined {
        if (typeof printingCondition.condition === "object")
            return resolvedConditions.get(printingCondition.condition);
        else if (printingCondition.condition instanceof Function) {
            return printingCondition.condition({
                getResolvedCondition,
                writerInfo: getWriterInfo(),
                getResolvedInfo
            });
        }
        else {
            return assertNever(printingCondition.condition);
        }

        function getResolvedCondition(condition: Condition): boolean | undefined {
            if (!resolvedConditions.has(condition) && !lookAheadSavePoints.has(condition)) {
                const savePoint = createSavePointForRestoringCondition(condition.name);
                lookAheadSavePoints.set(condition, savePoint);
                refreshWriterUseCommittedItems();
            }
            return resolvedConditions.get(condition);
        }

        function getResolvedInfo(info: Info) {
            const resolvedInfo = resolvedInfos.get(info);
            if (resolvedInfo == null && !lookAheadSavePoints.has(info)) {
                const savePoint = createSavePointForRestoringCondition(info.name);
                lookAheadSavePoints.set(info, savePoint);
                refreshWriterUseCommittedItems();
            }
            return resolvedInfo;
        }
    }

    function handleInfo(info: Info) {
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
            newLineGroupDepth,
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
