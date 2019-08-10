import { ResolveConditionContext, Info, WriterInfo, PrintItemKind } from "../types";

export namespace conditionResolvers {
    export function isStartOfNewLine(conditionContext: ResolveConditionContext) {
        return conditionContext.writerInfo.columnNumber === conditionContext.writerInfo.lineStartColumnNumber;
    }

    export function isHanging(conditionContext: ResolveConditionContext, startInfo: Info, endInfo?: Info) {
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
    }

    export function isMultipleLines(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info | WriterInfo, defaultValue?: boolean) {
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

    export function areInfoEqual(conditionContext: ResolveConditionContext, startInfo: Info, endInfo: Info, defaultValue: boolean) {
        const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
        const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);

        if (resolvedStartInfo == null || resolvedEndInfo == null)
            return defaultValue;

        return resolvedStartInfo.lineNumber === resolvedEndInfo.lineNumber
            && resolvedStartInfo.columnNumber === resolvedEndInfo.columnNumber;
    }
}
