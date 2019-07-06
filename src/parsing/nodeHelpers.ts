import * as babel from "@babel/types";

export function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

export function hasSeparatingBlankLine(nodeA: babel.Node, nodeB: babel.Node | undefined) {
    if (nodeB == null)
        return false;

    return getNodeBStartLine() > nodeA.loc!.end.line + 1;

    function getNodeBStartLine() {
        const leadingComments = nodeB!.leadingComments;

        if (leadingComments != null) {
            for (const leadingComment of leadingComments) {
                const commentStartLine = leadingComment.loc!.start.line;
                if (commentStartLine > nodeA.loc!.end.line)
                    return commentStartLine;
            }
        }

        return nodeB!.loc!.start.line;
    }
}

export function getLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    if (node.leadingComments == null)
        return undefined;

    for (const comment of node.leadingComments) {
        if (commentsToIgnore != null && commentsToIgnore.includes(comment))
            continue;

        if (comment.loc!.start.line < node.loc!.start.line)
            return comment;
    }

    return undefined;
}

export function hasLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    return getLeadingCommentOnDifferentLine(node, commentsToIgnore) != null;
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