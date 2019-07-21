import { Info, ResolveConditionContext, WriterInfo, PrintItemKind } from "../types";

export function isMultipleLines(startInfo: Info, endInfo: Info | WriterInfo, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = getResolvedEndInfo();
    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;
    return resolvedEndInfo.lineNumber > resolvedStartInfo.lineNumber;

    function getResolvedEndInfo() {
        if ((endInfo as any as Info).kind === PrintItemKind.Info)
            return conditionContext.getResolvedInfo(endInfo as Info);
        return endInfo as WriterInfo;
    }
}

export function areInfoEqual(startInfo: Info, endInfo: Info, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);

    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;

    return resolvedStartInfo.lineNumber === resolvedEndInfo.lineNumber
        && resolvedStartInfo.columnNumber === resolvedEndInfo.columnNumber;
}
