import * as babel from "@babel/types";
import { BaseContext } from "@dprint/core";
import { BabelToken } from "./BabelToken";

export function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

export function hasSeparatingBlankLine(nodeA: babel.Node | babel.Comment, nodeB: babel.Node | babel.Comment | undefined) {
    if (nodeB == null)
        return false;

    return getNodeBStartLine() > nodeA.loc!.end.line + 1;

    function getNodeBStartLine() {
        const leadingComments = (nodeB! as babel.Node).leadingComments;

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

export function getUseNewlinesForNodes(nodes: ReadonlyArray<babel.Node | BabelToken | null | undefined>) {
    const nonNullNodes = getNodes();
    const firstNode = nonNullNodes.next().value;
    const secondNode = nonNullNodes.next().value;

    if (firstNode == null || secondNode == null || firstNode.loc!.end.line === secondNode.loc!.start.line)
        return false;

    return true;

    function* getNodes() {
        for (const node of nodes) {
            if (node != null)
                yield node;
        }
    }
}

export function isFirstNodeOnLine(node: babel.Node | BabelToken, context: BaseContext) {
    for (let i = node.start! - 1; i >= 0; i--) {
        const char = context.fileText[i];
        if (char === " " || char === "\t")
            continue;

        return char === "\n";
    }

    return true;
}

export function hasParentheses(node: babel.Node): boolean {
    const extra = (node as any).extra;
    if (extra == null)
        return false;
    return extra.parenthesized || false;
}

export function getStartOrParenStart(node: babel.Node): number {
    const extra = (node as any).extra;
    const parenStart = extra && extra.parenStart;
    return parenStart != null ? parenStart : node.start!;
}

export function getJsxText(jsxText: babel.JSXText) {
    // this is necessary because .value will resolve character entities (ex. &nbsp; -> space)
    return (jsxText as any).extra.raw as string;
}
