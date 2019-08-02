import { PrintItemKind, Info, Condition, Signal } from "../types";
import { Context } from "./typescript/parser";
import * as infoChecks from "./infoChecks";
import * as conditionResolvers from "./conditionResolvers";

// reusable conditions

export function newlineIfHangingSpaceOtherwise(context: Context, startInfo: Info, endInfo?: Info, spaceChar: " " | Signal.SpaceOrNewLine = " "): Condition {
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

export function newlineIfMultipleLinesSpaceOrNewlineOtherwise(context: Context, startInfo: Info, endInfo?: Info): Condition {
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
