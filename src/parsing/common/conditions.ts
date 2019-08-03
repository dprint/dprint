import { PrintItemKind, Info, Condition, Signal, PrintItemIterator } from "../../types";
import { BaseContext } from "./BaseContext";
import * as infoChecks from "./infoChecks";
import * as conditionResolvers from "./conditionResolvers";
import { RepeatableIterator } from "../../utils";
import { withIndent } from "./parserHelpers";

// reusable conditions

export function newlineIfHangingSpaceOtherwise(
    context: BaseContext, startInfo: Info, endInfo?: Info, spaceChar: " " | Signal.SpaceOrNewLine = " "): Condition {
    return {
        kind: PrintItemKind.Condition,
        name: "newLineIfHangingSpaceOtherwise",
        condition: conditionContext => {
            const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
            if (resolvedStartInfo == null)
                return undefined;

            const resolvedEndInfo = getResolvedEndInfo();
            if (resolvedEndInfo == null)
                return undefined;

            return resolvedEndInfo.lineStartIndentLevel > resolvedStartInfo.lineStartIndentLevel;

            function getResolvedEndInfo() {
                if (endInfo == null)
                    return conditionContext.writerInfo; // use the current condition position

                // otherwise, use the end info
                const resolvedInfo = conditionContext.getResolvedInfo(endInfo);
                if (resolvedInfo == null)
                    return undefined;
                return resolvedInfo;
            }
        },
        true: [context.newlineKind],
        false: [spaceChar]
    };
}

export function newlineIfMultipleLinesSpaceOrNewlineOtherwise(context: BaseContext, startInfo: Info, endInfo?: Info): Condition {
    return {
        name: "newlineIfMultipleLinesSpaceOrNewlineOtherwise",
        kind: PrintItemKind.Condition,
        condition: conditionContext => infoChecks.isMultipleLines(startInfo, endInfo || conditionContext.writerInfo, conditionContext, false),
        true: [context.newlineKind],
        false: [Signal.SpaceOrNewLine]
    };
}

export function singleIndentIfStartOfLine(): Condition {
    return {
        kind: PrintItemKind.Condition,
        name: "singleIndentIfStartOfLine",
        condition: conditionResolvers.isStartOfNewLine,
        true: [Signal.SingleIndent]
    };
}

export function* indentIfStartOfLine(item: PrintItemIterator): PrintItemIterator {
    // need to make this a repeatable iterator so it can be iterated multiple times
    // between the true and false condition
    item = new RepeatableIterator(item);

    yield {
        kind: PrintItemKind.Condition,
        name: "indentIfStartOfLine",
        condition: conditionResolvers.isStartOfNewLine,
        true: withIndent(item),
        false: item
    };
}

export function* withIndentIfStartOfLineIndented(item: PrintItemIterator): PrintItemIterator {
    // need to make this a repeatable iterator so it can be iterated multiple times
    // between the true and false condition
    item = new RepeatableIterator(item);

    yield {
        kind: PrintItemKind.Condition,
        name: "withIndentIfStartOfLineIndented",
        condition: context => {
            return context.writerInfo.lineStartIndentLevel > context.writerInfo.indentLevel;
        },
        true: withIndent(item),
        false: item
    };
}
