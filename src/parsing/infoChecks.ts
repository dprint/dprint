import { Info, ResolveConditionContext } from "../types";

export function isMultipleLines(startInfo: Info, endInfo: Info, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);
    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;
    return resolvedEndInfo.lineNumber > resolvedStartInfo.lineNumber;
}

export function areInfoEqual(startInfo: Info, endInfo: Info, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);

    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;

    return resolvedStartInfo.lineNumber === resolvedEndInfo.lineNumber
        && resolvedStartInfo.columnNumber === resolvedEndInfo.columnNumber;
}
