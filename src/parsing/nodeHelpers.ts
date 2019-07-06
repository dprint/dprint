import * as babel from "@babel/types";

export function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

export function hasSeparatingBlankLine(nodeA: babel.Node, nodeB: babel.Node | undefined) {
    if (nodeB == null)
        return false;

    return nodeB.loc!.start.line > nodeA.loc!.end.line + 1;
}

export function hasLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    return node.leadingComments != null
        && node.leadingComments.some(c => {
            if (commentsToIgnore != null && commentsToIgnore.includes(c))
                return false;

            return c.type === "CommentLine" || c.loc!.start.line < node.loc!.start.line;
        });
}

export function useNewLinesForParametersOrArguments(params: babel.Node[]) {
    return getUseNewLinesForNodes(params);
}

export function getUseNewLinesForNodes(nodes: babel.Node[]) {
    if (nodes.length <= 1)
        return false;
    if (nodes[0].loc!.start.line === nodes[1].loc!.start.line)
        return false;
    return true;
}