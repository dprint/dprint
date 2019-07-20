import { PrintItemKind, Info, Condition, Signal } from "../types";
import { Context } from "./parser";
import * as infoChecks from "./infoChecks";

// reusable conditions

export function newlineIfHangingSpaceOtherwise(context: Context, info: Info): Condition {
    return {
        kind: PrintItemKind.Condition,
        name: "newLineIfHangingSpaceOtherwise",
        condition: conditionContext => {
            const resolvedInfo = conditionContext.getResolvedInfo(info);
            if (resolvedInfo == null)
                return undefined;
            const isHanging = conditionContext.writerInfo.lineStartIndentLevel > resolvedInfo.lineStartIndentLevel;
            return isHanging;
        },
        true: [context.newlineKind],
        false: [" "]
    };
}

export function newlineIfMultipleLinesSpaceOrNewlineOtherwise(context: Context, startInfo: Info, endInfo: Info): Condition {
    return {
        name: "newlineIfMultipleLinesSpaceOrNewlineOtherwise",
        kind: PrintItemKind.Condition,
        condition: conditionContext => infoChecks.isMultipleLines(startInfo, endInfo, conditionContext, false),
        true: [context.newlineKind],
        false: [Signal.SpaceOrNewLine]
    };
}
