import { ResolveConditionContext } from "../types";

export function isStartOfNewLine(conditionContext: ResolveConditionContext) {
    return conditionContext.writerInfo.columnNumber === conditionContext.writerInfo.lineStartColumnNumber;
}
